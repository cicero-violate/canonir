//! WebSocket server — bridges the Chrome extension to the agent.
//!
//! Architecture:
//!   - Single shared WS connection from the extension (one background.js).
//!   - All messages tagged with tabId.
//!
//! Extension → Rust frames:
//!   { "type": "TAB_OPENED",     "tabId": n, "url": "...", "reqId"?: n }
//!   { "type": "TAB_CLOSED",     "tabId": n }
//!   { "type": "TAB_READY",      "tabId": n, "url": "...", "reqId"?: n }
//!   { "type": "INBOUND_MESSAGE","tabId": n, "payload": "..." }
//!   { "type": "PING" }                          ← keepalive, ignored
//!
//! Rust → Extension frames:
//!   { "type": "OPEN_TAB",       "url": "...", "reqId"?: n }
//!   { "type": "TURN",           "tabId": n, "text": "..." }
//!   { "type": "OUTBOUND_SUBMIT","tabId": n, "payload": { ... } }
//!
//! WsBridge is a cheap-clone handle for callers.

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::sse::{classify_frame, FrameResult};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const RESPONSE_TIMEOUT_SECS: u64 = 120;

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum WsBridgeError {
    NotConnected,
    NoTab,
    Timeout,
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

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

struct ServerState {
    /// Outbound channel to the single extension WS connection.
    /// None when the extension is not connected.
    out_tx: Option<mpsc::Sender<Message>>,

    /// tabId → accumulated SSE delta strings for the in-flight request.
    tab_buffers: HashMap<u32, Vec<String>>,

    /// tabId → oneshot waiting for a completed response.
    pending: HashMap<u32, oneshot::Sender<String>>,

    /// reqId → oneshot waiting for TAB_OPENED confirmation.
    pending_open: HashMap<u64, oneshot::Sender<u32>>,

    /// All live tabs reported by the extension.
    live_tabs: std::collections::HashSet<u32>,

    /// TURN frames buffered while the extension WS is disconnected.
    /// Drained into out_tx the moment a new connection is established,
    /// so no TURN is silently dropped during a reconnect window.
    turn_replay_queue: Vec<Value>,

    /// Monotonic counter for frame dump filenames.
    frame_counter: u64,
}

impl ServerState {
    fn new() -> Self {
        // Prepare clean frame dump directory
        let dump_dir = "./frames";
        let _ = fs::create_dir_all(dump_dir);
        if let Ok(entries) = fs::read_dir(dump_dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }

        Self {
            out_tx: None,
            tab_buffers: HashMap::new(),
            pending: HashMap::new(),
            pending_open: HashMap::new(),
            live_tabs: std::collections::HashSet::new(),
            turn_replay_queue: Vec::new(),
            frame_counter: 0,
        }
    }

    fn send(&self, msg: Value) -> Result<(), WsBridgeError> {
        let tx = self.out_tx.as_ref().ok_or(WsBridgeError::NotConnected)?;
        let raw = msg.to_string();
        match tx.try_send(Message::Text(raw.into())) {
            Ok(()) => Ok(()),
            Err(e) => Err(WsBridgeError::NotConnected),
        }
    }
}

// ---------------------------------------------------------------------------
// Public bridge handle
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WsBridge {
    state: Arc<Mutex<ServerState>>,
    next_req_id: Arc<AtomicU64>,
}

impl WsBridge {
    /// Open a new tab at `url` and return its tabId once the extension confirms.
    pub async fn open_fresh_tab_with_url(&self, url: String) -> Result<u32, WsBridgeError> {
        let req_id = self.next_req_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<u32>();

        {
            let mut st = self.state.lock().await;
            st.pending_open.insert(req_id, tx);
            st.send(json!({ "type": "OPEN_TAB", "url": url, "reqId": req_id }))?;
        }

        rx.await.map_err(|_| WsBridgeError::Cancelled)
    }

    /// Send a TURN to `tab_id` and wait for the assembled response.
    ///
    /// If the WS is live the frame is sent immediately.
    /// If the WS is disconnected the frame is pushed into `turn_replay_queue`
    /// and will be replayed the moment the extension reconnects, so no TURN
    /// is silently lost during the ~1 s reconnect window.
    pub async fn send_turn(&self, tab_id: u32, text: String) -> Result<String, WsBridgeError> {
        let (tx, rx) = oneshot::channel::<String>();

        {
            let mut st = self.state.lock().await;
            st.pending.insert(tab_id, tx);
            st.tab_buffers.insert(tab_id, Vec::new());

            let frame = json!({ "type": "TURN", "tabId": tab_id, "text": text });

            match st.send(frame.clone()) {
                Ok(()) => {}
                Err(_) => {
                    // Socket is down — buffer the frame for replay on reconnect.
                    st.turn_replay_queue.push(frame);
                }
            }
        }

        match tokio::time::timeout(std::time::Duration::from_secs(RESPONSE_TIMEOUT_SECS), rx).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(_)) => Err(WsBridgeError::Cancelled),
            Err(_) => Err(WsBridgeError::Timeout),
        }
    }

    /// Block until the extension WS is connected.
    pub async fn wait_for_connection(&self) {
        loop {
            {
                let st = self.state.lock().await;
                if st.out_tx.is_some() {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    /// Send an OUTBOUND_SUBMIT directly to a tab (Rust-driven injection).
    pub async fn outbound_submit(&self, tab_id: u32, payload: Value) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        st.send(json!({ "type": "OUTBOUND_SUBMIT", "tabId": tab_id, "payload": payload }))
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

pub fn spawn(addr: SocketAddr) -> WsBridge {
    let state = Arc::new(Mutex::new(ServerState::new()));
    let bridge = WsBridge { state: state.clone(), next_req_id: Arc::new(AtomicU64::new(1)) };

    tokio::spawn(async move {
        loop {
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    accept_loop(listener, state.clone()).await;
                }
                Err(e) => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });

    bridge
}

async fn accept_loop(listener: TcpListener, state: Arc<Mutex<ServerState>>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                handle_connection(stream, state.clone()).await;
            }
            Err(_e) => {}
        }
    }
}

async fn handle_connection(stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(_e) => {
            return;
        }
    };

    let (mut sink, mut source) = ws.split();
    let (tx_out, mut rx_out) = mpsc::channel::<Message>(256);

    {
        let mut st = state.lock().await;
        st.out_tx = Some(tx_out.clone());

        // Drain any TURN frames that were buffered while the socket was down.
        let queued: Vec<Value> = st.turn_replay_queue.drain(..).collect();
        if !queued.is_empty() {
            for frame in queued {
                let _ = tx_out.try_send(Message::Text(frame.to_string().into()));
            }
        }
    }

    let sink_task = tokio::spawn(async move {
        while let Some(msg) = rx_out.recv().await {
            match sink.send(msg).await {
                Ok(()) => {}
                Err(_e) => {
                    break;
                }
            }
        }
    });

    while let Some(result) = source.next().await {
        match result {
            Ok(Message::Text(text)) => handle_inbound(text.as_str(), &state).await,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    sink_task.abort();

    {
        let mut st = state.lock().await;
        st.out_tx = None;
    }
}

// ---------------------------------------------------------------------------
// Inbound frame handler
// ---------------------------------------------------------------------------

async fn handle_inbound(raw: &str, state: &Arc<Mutex<ServerState>>) {
    let msg: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => {
            return;
        }
    };

    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        // Keepalive from background.js — no state change needed.
        "PING" => {}

        "TAB_OPENED" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let mut st = state.lock().await;
            st.live_tabs.insert(tab_id);
        }

        "TAB_CLOSED" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let mut st = state.lock().await;
            st.live_tabs.remove(&tab_id);
            st.tab_buffers.remove(&tab_id);
            st.pending.remove(&tab_id);
        }

        "TAB_READY" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let req_id = msg.get("reqId").and_then(|v| v.as_u64());

            let mut st = state.lock().await;

            if let Some(rid) = req_id {
                if let Some(tx) = st.pending_open.remove(&rid) {
                    let _ = tx.send(tab_id);
                }
            }
        }

        "INBOUND_MESSAGE" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let payload = match msg.get("payload").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return,
            };

            let mut st = state.lock().await;

            // Silence frame dumping

            match classify_frame(&payload) {
                FrameResult::Delta(text) => {
                    st.tab_buffers.entry(tab_id).or_default().push(text);
                }
                FrameResult::Snapshot(text) => {
                    // /gg path: full accumulated text — replace buffer and resolve immediately.
                    st.tab_buffers.insert(tab_id, vec![text.clone()]);
                    if let Some(tx) = st.pending.remove(&tab_id) {
                        let _ = tx.send(text);
                    }
                    st.tab_buffers.remove(&tab_id);
                }
                FrameResult::Done => {
                    if let (Some(buf), Some(tx)) = (st.tab_buffers.remove(&tab_id), st.pending.remove(&tab_id)) {
                        let assembled = buf.join("");
                        let _ = tx.send(assembled);
                    }
                }
                FrameResult::Ignore => {}
            }
        }

        _other => {}
    }
}
