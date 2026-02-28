//! WebSocket server — bridges the Chrome extension to the agent.
//!
//! Architecture:
//!   - Listens on 127.0.0.1:8787 for one persistent connection from the extension.
//!   - Inbound JSON frames from extension:
//!       { "type": "TAB_OPENED", "tabId": n }
//!       { "type": "TAB_CLOSED", "tabId": n }
//!       { "type": "CHUNK", "tabId": n, "raw": "..." }
//!   - Outbound JSON frames to extension:
//!       { "type": "OPEN_TAB" }
//!       { "type": "CLOSE_TAB", "tabId": n }
//!       { "type": "TURN", "tabId": n, "text": "..." }
//!
//!   - Per-tab chunk buffers accumulate raw SSE lines / WS frames.
//!   - When a chunk contains "data: [DONE]" the buffer is flushed and
//!     the waiting oneshot is resolved with the full assembled text.
//!
//!   - WsBridge is a cheap clone handle — callers use it to send turns
//!     and await responses without owning the server task.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use super::sse::{extract_sse_delta, is_done};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

/// How long to wait for a response before giving up.
const RESPONSE_TIMEOUT_SECS: u64 = 120;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Errors from the WS bridge.
#[derive(Debug)]
pub enum WsBridgeError {
    /// No extension connection is currently open.
    NotConnected,
    /// No live ChatGPT tab available.
    NoTab,
    /// Timed out waiting for a response from ChatGPT.
    Timeout,
    /// The response channel was dropped before a response arrived.
    Cancelled,
}

impl std::fmt::Display for WsBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsBridgeError::NotConnected => write!(f, "extension not connected"),
            WsBridgeError::NoTab => write!(f, "no live ChatGPT tab"),
            WsBridgeError::Timeout => write!(f, "timeout waiting for ChatGPT response"),
            WsBridgeError::Cancelled => write!(f, "response channel cancelled"),
        }
    }
}

impl std::error::Error for WsBridgeError {}

/// Shared state — one instance, protected by a Mutex.
struct ServerState {
    /// tabId → accumulated raw chunks for the in-flight request.
    tab_buffers: HashMap<u32, Vec<String>>,
    /// tabId → oneshot sender waiting for a completed response.
    pending: HashMap<u32, oneshot::Sender<String>>,
    /// Single controlled ChatGPT tab.
    controlled_tab: Option<u32>,
    /// All active WS connections (conn_id → tx)
    connections: HashMap<u64, mpsc::Sender<Message>>,

    /// Authoritative control-plane connection
    control_conn: Option<u64>,

    /// req_id → oneshot sender for OPEN_TAB correlation
    pending_open: HashMap<u64, oneshot::Sender<u32>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            tab_buffers: HashMap::new(),
            pending: HashMap::new(),
            controlled_tab: None,
            connections: HashMap::new(),
            control_conn: None,
            pending_open: HashMap::new(),
        }
    }

    fn controlled(&self) -> Option<u32> {
        self.controlled_tab
    }
}

/// Cheap-clone handle to the WS server. Hand this to `call_llm`.
#[derive(Clone)]
pub struct WsBridge {
    state: Arc<Mutex<ServerState>>,
    next_req_id: Arc<AtomicU64>,
}

impl WsBridge {
    /// Send a TURN to the given tab (or first live tab if tabId is None)
    /// and wait for the assembled response.
    pub async fn send_turn(&self, tab_id: Option<u32>, text: String) -> Result<String, WsBridgeError> {
        let (tx, rx) = oneshot::channel::<String>();

        let target = {
            let mut st = self.state.lock().await;

            let target = match tab_id.or_else(|| st.controlled()) {
                Some(id) => id,
                None => return Err(WsBridgeError::NoTab),
            };

            let control = st.control_conn.ok_or(WsBridgeError::NotConnected)?;
            let out_tx = st.connections.get(&control)
                .cloned()
                .ok_or(WsBridgeError::NotConnected)?;

            // Register the oneshot before sending so no chunk is missed.
            st.pending.insert(target, tx);
            st.tab_buffers.insert(target, Vec::new());

            let frame = json!({ "type": "TURN", "tabId": target, "text": text });
            out_tx.send(Message::Text(frame.to_string().into())).await.map_err(|_| WsBridgeError::NotConnected)?;

            target
        };

        eprintln!("[ws] TURN sent to tab {target}");

        // Wait for the assembled response with a timeout.
        match tokio::time::timeout(std::time::Duration::from_secs(RESPONSE_TIMEOUT_SECS), rx).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(_)) => Err(WsBridgeError::Cancelled),
            Err(_) => Err(WsBridgeError::Timeout),
        }
    }

    /// Send an OPEN_TAB command to the extension.
    pub async fn open_tab(&self) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        let control = st.control_conn.ok_or(WsBridgeError::NotConnected)?;
        let tx = st.connections.get(&control)
            .cloned()
            .ok_or(WsBridgeError::NotConnected)?;
        let frame = json!({ "type": "OPEN_TAB" });
        tx.send(Message::Text(frame.to_string().into())).await.map_err(|_| WsBridgeError::NotConnected)
    }

    /// Send an OPEN_TAB command with explicit URL.
    pub async fn open_tab_with_url(&self, url: String) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        let control = st.control_conn.ok_or(WsBridgeError::NotConnected)?;
        let tx = st.connections.get(&control)
            .cloned()
            .ok_or(WsBridgeError::NotConnected)?;
        let frame = json!({ "type": "OPEN_TAB", "url": url });
        tx.send(Message::Text(frame.to_string().into()))
            .await
            .map_err(|_| WsBridgeError::NotConnected)
    }

    /// Send a CLOSE_TAB command to the extension.
    pub async fn close_tab(&self, tab_id: u32) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        let control = st.control_conn.ok_or(WsBridgeError::NotConnected)?;
        let tx = st.connections.get(&control)
            .cloned()
            .ok_or(WsBridgeError::NotConnected)?;
        let frame = json!({ "type": "CLOSE_TAB", "tabId": tab_id });
        tx.send(Message::Text(frame.to_string().into())).await.map_err(|_| WsBridgeError::NotConnected)
    }

    /// Block until the extension has an open WS connection.
    pub async fn wait_for_connection(&self) {
        loop {
            {
                let st = self.state.lock().await;
                if !st.connections.is_empty() {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    // wait_for_tab removed — deterministic single-tab ownership

    // open_fresh_tab removed — use open_fresh_tab_with_url with reqId correlation

    /// Open a fresh tab with explicit URL and return its tab_id.
    pub async fn open_fresh_tab_with_url(&self, url: String) -> Result<u32, WsBridgeError> {
        let req_id = self.next_req_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<u32>();

        {
            let mut st = self.state.lock().await;
            st.pending_open.insert(req_id, tx);
        }

        let tx_out = {
            let st = self.state.lock().await;
            let control = st.control_conn.ok_or(WsBridgeError::NotConnected)?;
            st.connections.get(&control)
                .cloned()
                .ok_or(WsBridgeError::NotConnected)?
        };

        let frame = json!({
            "type": "OPEN_TAB",
            "url": url,
            "reqId": req_id
        });

        tx_out
            .send(Message::Text(frame.to_string().into()))
            .await
            .map_err(|_| WsBridgeError::NotConnected)?;

        rx.await.map_err(|_| WsBridgeError::Cancelled)
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

/// Spawn the WS server as a background tokio task.
/// Returns a WsBridge handle immediately — the server runs concurrently.
pub fn spawn(addr: SocketAddr) -> WsBridge {
    let state = Arc::new(Mutex::new(ServerState::new()));
    let bridge = WsBridge {
        state: state.clone(),
        next_req_id: Arc::new(AtomicU64::new(1)),
    };

    tokio::spawn(async move {
        loop {
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    eprintln!("[ws] listening on {addr}");
                    accept_loop(listener, state.clone()).await;
                }
                Err(e) => {
                    eprintln!("[ws] bind error: {e} — retrying in 2s");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });

    bridge
}

/// Accepts one connection at a time. When the connection drops,
/// returns so the outer loop can accept a new one.
async fn accept_loop(listener: TcpListener, state: Arc<Mutex<ServerState>>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                eprintln!("[ws] extension connected from {peer}");
                handle_connection(stream, state.clone()).await;
                eprintln!("[ws] extension disconnected");

                // Connection cleanup handled inside handle_connection
            }
            Err(e) => {
                eprintln!("[ws] accept error: {e}");
            }
        }
    }
}

/// Drive one WebSocket connection to completion.
async fn handle_connection(stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("[ws] handshake error: {e}");
            return;
        }
    };

    let (mut sink, mut source) = ws.split();

    // Outbound channel: server state → sink task → extension.
    let (tx_out, mut rx_out) = mpsc::channel::<Message>(64);

    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);

    {
        let mut st = state.lock().await;
        st.connections.insert(conn_id, tx_out);

        // Elect first connection as control-plane
        if st.control_conn.is_none() {
            st.control_conn = Some(conn_id);
        }
    }

    // Sink task: drains rx_out into the WS sink.
    let sink_task = tokio::spawn(async move {
        while let Some(msg) = rx_out.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Source loop: process inbound frames from extension.
    while let Some(result) = source.next().await {
        match result {
            Ok(Message::Text(text)) => {
                handle_inbound(text.as_str(), &state).await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    sink_task.abort();

    // Cleanup connection on disconnect
    {
        let mut st = state.lock().await;
        st.connections.remove(&conn_id);

        if st.control_conn == Some(conn_id) {
            st.control_conn = st.connections.keys().next().copied();
        }
    }
}

// ---------------------------------------------------------------------------
// Inbound frame handler
// ---------------------------------------------------------------------------

async fn handle_inbound(raw: &str, state: &Arc<Mutex<ServerState>>) {
    let msg: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[ws] non-JSON frame: {}", &raw[..raw.len().min(120)]);
            return;
        }
    };

    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "TAB_OPENED" => {
            if let Some(tab_id) = msg.get("tabId").and_then(|v| v.as_u64()) {
                let tab_id = tab_id as u32;
                let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let req_id = msg.get("reqId").and_then(|v| v.as_u64());
                let mut st = state.lock().await;
                // Boot phase: first valid OPEN_TAB response becomes controlled tab
                if st.controlled_tab.is_none() {
                    st.controlled_tab = Some(tab_id);
                }

                if let Some(rid) = req_id {
                    if let Some(tx) = st.pending_open.remove(&rid) {
                        let _ = tx.send(tab_id);
                    }
                }

                eprintln!("[ws] tab opened: {tab_id} url={url}");
            }
        }

        "TAB_CLOSED" => {
            if let Some(tab_id) = msg.get("tabId").and_then(|v| v.as_u64()) {
                let tab_id = tab_id as u32;
                let mut st = state.lock().await;
                if st.controlled_tab == Some(tab_id) {
                    st.controlled_tab = None;
                }
                st.tab_buffers.remove(&tab_id);
                st.pending.remove(&tab_id);
                eprintln!("[ws] tab closed: {tab_id}");
            }
        }

    "CHUNK" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let raw_chunk = match msg.get("raw").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };

            let done = is_done(&raw_chunk);
            let extracted = extract_sse_delta(&raw_chunk);

            let mut st = state.lock().await;

            if let Some(text) = extracted {
                st.tab_buffers.entry(tab_id).or_default().push(text);
            }

            if done {
                if let (Some(buffer), Some(tx)) = (st.tab_buffers.remove(&tab_id), st.pending.remove(&tab_id)) {
                    let assembled = buffer.join("");
                    eprintln!("[ws] tab {tab_id} done — {} bytes", assembled.len());
                    let _ = tx.send(assembled);
                }
            }
        }

        other => {
            eprintln!("[ws] unknown frame type: {other}");
            eprintln!("[ws] full frame: {}", msg);
        }
    }
}
