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
//!   { "type": "TURN",           "tabId": n, "text": "...", "turnId"?: n }
//!   { "type": "OUTBOUND_SUBMIT","tabId": n, "payload": { ... } }
//!   { "type": "CLOSE_TAB",      "tabId": n }
//!   { "type": "NEW_CHAT",       "tabId": n }
//!   { "type": "TEMP_CHAT",      "tabId": n }
//!
//! WsBridge is a cheap-clone handle for callers.

use canon_event::EventEmitterHandle;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use super::parsers::{FrameAssembler, SiteType};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

fn append_jsonl(path: &str, value: &Value) {
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(value) {
            let _ = writeln!(file, "{}", line);
        }
    }
}

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

    /// tabId → frame assembler (parser + buffering).
    tab_assemblers: HashMap<u32, FrameAssembler>,

    /// tabId → oneshot waiting for a completed response.
    pending: HashMap<u32, oneshot::Sender<String>>,
    /// tabId → expected turnId for inbound chunk filtering.
    pending_turn_id: HashMap<u32, u64>,

    /// reqId → oneshot waiting for TAB_OPENED confirmation.
    pending_open: HashMap<u64, oneshot::Sender<u32>>,
    /// tabId → oneshot waiting for NEW_CHAT completion.
    pending_new_chat: HashMap<u32, oneshot::Sender<()>>,
    /// tabId → oneshot waiting for TEMP_CHAT completion.
    pending_temp_chat: HashMap<u32, oneshot::Sender<()>>,

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
            tab_assemblers: HashMap::new(),
            pending: HashMap::new(),
            pending_turn_id: HashMap::new(),
            pending_open: HashMap::new(),
            pending_new_chat: HashMap::new(),
            pending_temp_chat: HashMap::new(),
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
            Err(_e) => Err(WsBridgeError::NotConnected),
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
    next_turn_id: Arc<AtomicU64>,
    response_timeout_secs: u64,
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
    pub async fn send_turn(&self, tab_id: u32, url: &str, text: String) -> Result<String, WsBridgeError> {
        let (tx, rx) = oneshot::channel::<String>();
        let turn_id = self.next_turn_id.fetch_add(1, Ordering::Relaxed);

        {
            let mut st = self.state.lock().await;
            st.pending.insert(tab_id, tx);
            st.pending_turn_id.insert(tab_id, turn_id);
            if !st.tab_assemblers.contains_key(&tab_id) {
                let site = SiteType::from_url(url);
                st.tab_assemblers.insert(tab_id, FrameAssembler::new(site));
            } else if let Some(asm) = st.tab_assemblers.get_mut(&tab_id) {
                asm.reset();
            }

            let frame = json!({ "type": "TURN", "tabId": tab_id, "text": text, "turnId": turn_id });

            match st.send(frame.clone()) {
                Ok(()) => {}
                Err(_) => {
                    // Socket is down — buffer the frame for replay on reconnect.
                    st.turn_replay_queue.push(frame);
                }
            }
        }

        match tokio::time::timeout(std::time::Duration::from_secs(self.response_timeout_secs), rx).await {
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

    /// Close a tab by id.
    pub async fn close_tab(&self, tab_id: u32) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        st.send(json!({ "type": "CLOSE_TAB", "tabId": tab_id }))
    }

    /// Trigger a new chat in the tab.
    pub async fn new_chat(&self, tab_id: u32) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        st.send(json!({ "type": "NEW_CHAT", "tabId": tab_id }))
    }

    /// Trigger temporary chat in the tab (ChatGPT).
    pub async fn temp_chat(&self, tab_id: u32) -> Result<(), WsBridgeError> {
        let st = self.state.lock().await;
        st.send(json!({ "type": "TEMP_CHAT", "tabId": tab_id }))
    }

    /// Wait for NEW_CHAT completion signal from the tab.
    pub async fn wait_new_chat(&self, tab_id: u32, timeout_secs: u64) -> Result<(), WsBridgeError> {
        let (tx, rx) = oneshot::channel::<()>();
        {
            let mut st = self.state.lock().await;
            st.pending_new_chat.insert(tab_id, tx);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(WsBridgeError::Cancelled),
            Err(_) => Err(WsBridgeError::Timeout),
        }
    }

    /// Wait for TEMP_CHAT completion signal from the tab.
    pub async fn wait_temp_chat(&self, tab_id: u32, timeout_secs: u64) -> Result<(), WsBridgeError> {
        let (tx, rx) = oneshot::channel::<()>();
        {
            let mut st = self.state.lock().await;
            st.pending_temp_chat.insert(tab_id, tx);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(WsBridgeError::Cancelled),
            Err(_) => Err(WsBridgeError::Timeout),
        }
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

/// Spawn the WS bridge server.
///
/// `emitter` is an `Arc<OnceLock<EventEmitterHandle>>` shared with the LLM
/// executor worker.  It is populated by the first LLM job so ws_server can
/// emit bridge-level events (connection, tab lifecycle) as P→Q producers
/// for the full process lifetime — independent of any single capability request.
pub fn spawn(addr: SocketAddr, response_timeout_secs: u64, _emitter: Arc<OnceLock<EventEmitterHandle>>) -> WsBridge {
    let state = Arc::new(Mutex::new(ServerState::new()));
    let bridge = WsBridge { state: state.clone(), next_req_id: Arc::new(AtomicU64::new(1)), next_turn_id: Arc::new(AtomicU64::new(1)), response_timeout_secs };

    tokio::spawn(async move {
        loop {
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    accept_loop(listener, state.clone()).await;
                }
                Err(_e) => {
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
            Ok((stream, _peer)) => {
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
        for frame in st.turn_replay_queue.drain(..) {
            let _ = tx_out.try_send(Message::Text(frame.to_string().into()));
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
            let site = SiteType::from_url(url);
            st.tab_assemblers.entry(tab_id).and_modify(|asm| asm.set_site(site)).or_insert_with(|| FrameAssembler::new(site));
        }

        "TAB_CLOSED" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let mut st = state.lock().await;
            st.live_tabs.remove(&tab_id);
            st.tab_assemblers.remove(&tab_id);
            st.pending.remove(&tab_id);
            st.pending_turn_id.remove(&tab_id);
            st.pending_new_chat.remove(&tab_id);
        }

        "TAB_READY" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let url = msg.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let req_id = msg.get("reqId").and_then(|v| v.as_u64());

            let mut st = state.lock().await;
            let site = SiteType::from_url(url);
            st.tab_assemblers.entry(tab_id).and_modify(|asm| asm.set_site(site)).or_insert_with(|| FrameAssembler::new(site));

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
            let mut inbound_turn_id: Option<u64> = None;
            let mut chunk = payload.clone();
            if payload.trim_start().starts_with('{') {
                if let Ok(v) = serde_json::from_str::<Value>(&payload) {
                    if let Some(c) = v.get("chunk").and_then(|v| v.as_str()) {
                        chunk = c.to_string();
                    }
                    if let Some(tid) = v.get("turn_id").and_then(|v| v.as_u64()) {
                        inbound_turn_id = Some(tid);
                    }
                }
            }

            let expected = st.pending_turn_id.get(&tab_id).copied();
            if let Some(tid) = inbound_turn_id {
                if expected != Some(tid) {
                    return;
                }
            } else if expected.is_some() {
                // Accept chunks that do not include turn_id and bind to expected.
                inbound_turn_id = expected;
            }

            // Dump every inbound frame to disk for parser inspection.
            st.frame_counter += 1;
            let inbound_record = json!({
                "frame_counter": st.frame_counter,
                "tab_id": tab_id,
                "turn_id": inbound_turn_id,
                "expected_turn_id": expected,
                "chunk": chunk,
            });
            append_jsonl("./frames/inbound.jsonl", &inbound_record);

            let assembled = if let Some(asm) = st.tab_assemblers.get_mut(&tab_id) {
                asm.push(&chunk)
            } else {
                let mut asm = FrameAssembler::new(SiteType::Unknown);
                let out = asm.push(&chunk);
                st.tab_assemblers.insert(tab_id, asm);
                out
            };

            if let Some(text) = assembled {
                // Dump assembled message for debugging (Gemini/ChatGPT).
                let assembled_record = json!({
                    "tab_id": tab_id,
                    "turn_id": inbound_turn_id.or(expected),
                    "text": text,
                });
                append_jsonl("./frames/assembled.jsonl", &assembled_record);
                if let Some(tx) = st.pending.remove(&tab_id) {
                    let _ = tx.send(text);
                }
                st.pending_turn_id.remove(&tab_id);
            }
        }

        "NEW_CHAT_DONE" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let mut st = state.lock().await;
            if let Some(tx) = st.pending_new_chat.remove(&tab_id) {
                let _ = tx.send(());
            }
        }
        "TEMP_CHAT_DONE" => {
            let tab_id = match msg.get("tabId").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => return,
            };
            let mut st = state.lock().await;
            if let Some(tx) = st.pending_temp_chat.remove(&tab_id) {
                let _ = tx.send(());
            }
        }

        _other => {}
    }
}
