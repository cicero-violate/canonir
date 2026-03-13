use super::config::CapabilityConfig;
use super::response_router;
use super::tab_management::{
    tab_manager_drop_tab, tab_manager_get_or_open_tab, tab_manager_mark_tab_cooldown, tab_manager_mark_tab_in_flight, tab_manager_mark_tab_response, tab_manager_mark_tab_sent,
};
pub use super::tab_management::{tab_manager_log_llm, tab_manager_now_ms, TabManagerHandle};
use super::telemetry;
use crate::llm_domains::{is_chatgpt_url, is_gemini_url};
use crate::ws_server::WsBridge;
use anyhow::Result;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot, Mutex};
static WORKERS: Lazy<Mutex<HashMap<(String, bool), mpsc::Sender<LlmWorkItem>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);
pub fn llm_worker_new_tabs() -> TabManagerHandle {
    std::sync::Arc::new(tokio::sync::Mutex::new(super::tab_management::TabSlotTable::new()))
}
pub struct LlmWorkItem {
    pub req_id: u64,
    pub node_id: Option<String>,
    pub cache_key: Option<u64>,
    pub allow_req_id_mismatch: bool,
    pub prompt: String,
    pub role_schema: String,
    pub phase: String,
    pub response: oneshot::Sender<Result<String>>,
}
struct LlmWorker {
    endpoint_id: String,
    url: String,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    stateful: bool,
    bridge: WsBridge,
    tabs: TabManagerHandle,
    seen_hashes: HashSet<u64>,
    cache: HashMap<u64, String>,
}
impl LlmWorker {
    async fn handle_request(&mut self, req: LlmWorkItem) {
        telemetry::telemetry_inc_pending();
        if let Some(key) = req.cache_key {
            if let Some(hit) = self.cache.get(&key) {
                telemetry::telemetry_dec_pending();
                let _ = req.response.send(Ok(hit.clone()));
                return;
            }
        }
        let full_prompt = if req.role_schema.trim().is_empty() { req.prompt } else { format!("{}\n\n{}", req.role_schema.trim_end(), req.prompt) };
        let full_prompt = format!("[REQ_ID:{}]\n{}", req.req_id, full_prompt);
        let prompt_chars = full_prompt.len();
        let full_prompt = if prompt_chars > 120_000 {
            let truncated = &full_prompt[..120_000];
            let cut = truncated.rfind('\n').unwrap_or(120_000);
            let mut s = full_prompt[..cut].to_string();
            s.push_str("\n... [prompt truncated]\n");
            s
        } else {
            full_prompt
        };
        let _ = prompt_chars;
        if let Some(node_id) = req.node_id.as_deref() {
            response_router::response_router_register(req.req_id, node_id).await;
        }
        tab_manager_log_llm(format!("phase={} endpoint={} req_id={} send_turn_start", req.phase, self.endpoint_id, req.req_id));
        let result = self.send_turn(&req.phase, req.req_id, req.allow_req_id_mismatch, full_prompt).await;
        if let Ok(raw) = result.as_ref() {
            if let Some(key) = req.cache_key {
                self.cache.insert(key, raw.clone());
            }
        }
        telemetry::telemetry_dec_pending();
        let _ = req.response.send(result);
    }
    async fn send_turn(&mut self, phase: &str, req_id: u64, allow_req_id_mismatch: bool, full_prompt: String) -> Result<String> {
        let tab_id = tab_manager_get_or_open_tab(&self.bridge, &self.endpoint_id, &self.url, &self.tabs, self.max_tabs).await?;
        tab_manager_mark_tab_sent(&self.tabs, tab_id).await;
        tab_manager_log_llm(format!("phase={} endpoint={} tab={} send", phase, self.endpoint_id, tab_id));
        let raw = match self.bridge.send_turn(tab_id, &self.url, full_prompt).await {
            Ok(v) => v,
            Err(e) => {
                tab_manager_mark_tab_in_flight(&self.tabs, tab_id, false).await;
                tab_manager_drop_tab(&self.tabs, &self.endpoint_id, tab_id).await;
                tab_manager_log_llm(format!("phase={} endpoint={} tab={} send_error={}", phase, self.endpoint_id, tab_id, e));
                return Err(anyhow::anyhow!("llm send_turn error: {e}"));
            }
        };
        tab_manager_mark_tab_response(&self.tabs, tab_id).await;
        tab_manager_mark_tab_in_flight(&self.tabs, tab_id, false).await;
        tab_manager_log_llm(format!("phase={} endpoint={} tab={} response_ok bytes={}", phase, self.endpoint_id, tab_id, raw.len()));
        if !llm_worker_response_matches_req_id(&raw, req_id) {
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} req_id_mismatch expected={}", phase, self.endpoint_id, tab_id, req_id));
            if !allow_req_id_mismatch {
                return Err(anyhow::anyhow!("req_id mismatch"));
            }
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} req_id_mismatch_accepted", phase, self.endpoint_id, tab_id));
        }
        let _ = response_router::response_router_resolve(req_id).await;
        if !self.stateful && is_gemini_url(&self.url) {
            let _ = self.bridge.new_chat(tab_id).await;
            match self.bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, self.endpoint_id, tab_id)),
                Err(e) => {
                    tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
        } else if !self.stateful && is_chatgpt_url(&self.url) {
            let _ = self.bridge.new_chat(tab_id).await;
            match self.bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, self.endpoint_id, tab_id)),
                Err(e) => {
                    tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
            let _ = self.bridge.temp_chat(tab_id).await;
            match self.bridge.wait_temp_chat(tab_id, 20).await {
                Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} temp_chat_done", phase, self.endpoint_id, tab_id)),
                Err(e) => {
                    tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    tab_manager_log_llm(format!("phase={} endpoint={} tab={} temp_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("temp_chat timeout"));
                }
            }
        }
        if self.tab_cooldown_ms > 0 {
            tab_manager_mark_tab_cooldown(&self.tabs, tab_id, self.tab_cooldown_ms).await;
        }
        let hash = llm_worker_stable_hash64(&raw);
        if !self.seen_hashes.insert(hash) {
            // Duplicate outputs can legitimately occur for verify/observe steps; don't fail the call.
            tab_manager_log_llm(format!(
                "phase={} endpoint={} tab={} duplicate_hash={}",
                phase, self.endpoint_id, tab_id, hash
            ));
        }
        Ok(raw)
    }
}
pub async fn llm_worker_send_request(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, allow_req_id_mismatch: bool, phase: &str,
    tabs: &TabManagerHandle, max_tabs: usize, tab_cooldown_ms: u64,
) -> Result<String> {
    let (req_id, raw) = llm_worker_send_request_with_req_id(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        cache_key,
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let _ = req_id;
    Ok(raw)
}

pub async fn llm_worker_send_request_with_req_id(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, allow_req_id_mismatch: bool, phase: &str,
    tabs: &TabManagerHandle, max_tabs: usize, tab_cooldown_ms: u64,
) -> Result<(u64, String)> {
    let req_id = NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    let mut workers = WORKERS.lock().await;
    let worker_key = (endpoint_id.to_string(), stateful);
    let sender = if let Some(sender) = workers.get(&worker_key) {
        sender.clone()
    } else {
        let (tx_worker, rx_worker) = mpsc::channel(64);
        let worker = LlmWorker {
            endpoint_id: endpoint_id.to_string(),
            url: url.to_string(),
            max_tabs,
            tab_cooldown_ms,
            stateful,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: HashSet::new(),
            cache: HashMap::new(),
        };
        tokio::spawn(llm_worker_run_worker(worker, rx_worker));
        workers.insert(worker_key, tx_worker.clone());
        tx_worker
    };
    let req = LlmWorkItem {
        req_id,
        node_id: node_id.map(|v| v.to_string()),
        cache_key,
        allow_req_id_mismatch,
        prompt: prompt.to_string(),
        role_schema: role_schema.to_string(),
        phase: phase.to_string(),
        response: tx,
    };
    sender.send(req).await.map_err(|_| anyhow::anyhow!("endpoint worker closed"))?;
    let raw = rx.await.map_err(|_| anyhow::anyhow!("endpoint worker canceled"))??;
    Ok((req_id, raw))
}
pub async fn llm_worker_init_workers(bridge: &WsBridge, config: &CapabilityConfig, tabs: &TabManagerHandle) {
    let mut workers = WORKERS.lock().await;
    for endpoint in &config.llm_endpoints {
        let worker_key = (endpoint.id.clone(), endpoint.stateful);
        if workers.contains_key(&worker_key) {
            continue;
        }
        let (tx_worker, rx_worker) = mpsc::channel(64);
        let worker = LlmWorker {
            endpoint_id: endpoint.id.clone(),
            url: endpoint.url.clone(),
            max_tabs: endpoint.max_tabs,
            tab_cooldown_ms: config.tab_cooldown_ms,
            stateful: endpoint.stateful,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: HashSet::new(),
            cache: HashMap::new(),
        };
        tokio::spawn(llm_worker_run_worker(worker, rx_worker));
        workers.insert(worker_key, tx_worker);
    }
}
async fn llm_worker_run_worker(mut worker: LlmWorker, mut rx: mpsc::Receiver<LlmWorkItem>) {
    while let Some(req) = rx.recv().await {
        worker.handle_request(req).await;
    }
}
fn llm_worker_stable_hash64(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
fn llm_worker_response_matches_req_id(raw: &str, req_id: u64) -> bool {
    let needle = format!("[REQ_ID:{}]", req_id);
    raw.contains(&needle)
}
