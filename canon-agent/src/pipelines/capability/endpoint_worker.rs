use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use once_cell::sync::Lazy;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::llm_domains::{is_chatgpt_url, is_gemini_url};
use crate::ws_server::WsBridge;

use super::config::CapabilityConfig;
use super::response_router;
use super::telemetry;
use super::tab_management::{
    drop_tab,
    get_or_open_tab,
    log_llm,
    mark_tab_cooldown,
    mark_tab_in_flight,
    mark_tab_response,
    mark_tab_sent,
    TabsHandle,
};

static WORKERS: Lazy<Mutex<HashMap<String, mpsc::Sender<LlmRequest>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);

pub struct LlmRequest {
    pub req_id: u64,
    pub node_id: Option<String>,
    pub cache_key: Option<u64>,
    pub prompt: String,
    pub role_schema: String,
    pub phase: String,
    pub response: oneshot::Sender<Result<String>>,
}

struct EndpointWorker {
    endpoint_id: String,
    url: String,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    bridge: WsBridge,
    tabs: TabsHandle,
    seen_hashes: HashSet<u64>,
    cache: HashMap<u64, String>,
}

impl EndpointWorker {
    async fn handle_request(&mut self, req: LlmRequest) {
        telemetry::inc_pending();
        if let Some(key) = req.cache_key {
            if let Some(hit) = self.cache.get(&key) {
                telemetry::dec_pending();
                let _ = req.response.send(Ok(hit.clone()));
                return;
            }
        }
        let full_prompt = if req.role_schema.trim().is_empty() {
            req.prompt
        } else {
            format!("{}\n\n{}", req.role_schema.trim_end(), req.prompt)
        };
        let full_prompt = format!("[REQ_ID:{}]\n{}", req.req_id, full_prompt);
        if let Some(node_id) = req.node_id.as_deref() {
            response_router::register(req.req_id, node_id).await;
        }
        let result = self.send_turn(&req.phase, req.req_id, full_prompt).await;
        if let Ok(raw) = result.as_ref() {
            if let Some(key) = req.cache_key {
                self.cache.insert(key, raw.clone());
            }
        }
        telemetry::dec_pending();
        let _ = req.response.send(result);
    }

    async fn send_turn(&mut self, phase: &str, req_id: u64, full_prompt: String) -> Result<String> {
        let tab_id = get_or_open_tab(
            &self.bridge,
            &self.endpoint_id,
            &self.url,
            &self.tabs,
            self.max_tabs,
        )
        .await?;
        mark_tab_sent(&self.tabs, tab_id).await;
        log_llm(format!(
            "phase={} endpoint={} tab={} send",
            phase, self.endpoint_id, tab_id
        ));
        let raw = match self.bridge.send_turn(tab_id, full_prompt).await {
            Ok(v) => v,
            Err(e) => {
                mark_tab_in_flight(&self.tabs, tab_id, false).await;
                drop_tab(&self.tabs, &self.endpoint_id, tab_id).await;
                log_llm(format!(
                    "phase={} endpoint={} tab={} send_error={}",
                    phase, self.endpoint_id, tab_id, e
                ));
                return Err(anyhow::anyhow!("llm send_turn error: {e}"));
            }
        };
        mark_tab_response(&self.tabs, tab_id).await;
        mark_tab_in_flight(&self.tabs, tab_id, false).await;
        log_llm(format!(
            "phase={} endpoint={} tab={} response_ok bytes={}",
            phase,
            self.endpoint_id,
            tab_id,
            raw.len()
        ));

        if !response_matches_req_id(&raw, req_id) {
            log_llm(format!(
                "phase={} endpoint={} tab={} req_id_mismatch expected={}",
                phase, self.endpoint_id, tab_id, req_id
            ));
            return Err(anyhow::anyhow!("req_id mismatch"));
        }
        let _ = response_router::resolve(req_id).await;

        if is_gemini_url(&self.url) {
            let _ = self.bridge.new_chat(tab_id).await;
            match self.bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!(
                    "phase={} endpoint={} tab={} new_chat_done",
                    phase, self.endpoint_id, tab_id
                )),
                Err(e) => {
                    mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    log_llm(format!(
                        "phase={} endpoint={} tab={} new_chat_timeout={}",
                        phase, self.endpoint_id, tab_id, e
                    ));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
        } else if is_chatgpt_url(&self.url) {
            let _ = self.bridge.new_chat(tab_id).await;
            match self.bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!(
                    "phase={} endpoint={} tab={} new_chat_done",
                    phase, self.endpoint_id, tab_id
                )),
                Err(e) => {
                    mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    log_llm(format!(
                        "phase={} endpoint={} tab={} new_chat_timeout={}",
                        phase, self.endpoint_id, tab_id, e
                    ));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
            let _ = self.bridge.temp_chat(tab_id).await;
            match self.bridge.wait_temp_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!(
                    "phase={} endpoint={} tab={} temp_chat_done",
                    phase, self.endpoint_id, tab_id
                )),
                Err(e) => {
                    mark_tab_in_flight(&self.tabs, tab_id, true).await;
                    log_llm(format!(
                        "phase={} endpoint={} tab={} temp_chat_timeout={}",
                        phase, self.endpoint_id, tab_id, e
                    ));
                    return Err(anyhow::anyhow!("temp_chat timeout"));
                }
            }
        }

        if self.tab_cooldown_ms > 0 {
            mark_tab_cooldown(&self.tabs, tab_id, self.tab_cooldown_ms).await;
        }

        let hash = stable_hash64(&raw);
        if !self.seen_hashes.insert(hash) {
            log_llm(format!(
                "phase={} endpoint={} tab={} duplicate_hash={}",
                phase, self.endpoint_id, tab_id, hash
            ));
            return Err(anyhow::anyhow!("duplicate response hash"));
        }

        Ok(raw)
    }
}

pub async fn send_request(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    prompt: &str,
    role_schema: &str,
    node_id: Option<&str>,
    cache_key: Option<u64>,
    phase: &str,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
) -> Result<String> {
    let req_id = NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    let mut workers = WORKERS.lock().await;
    let sender = if let Some(sender) = workers.get(endpoint_id) {
        sender.clone()
    } else {
        let (tx_worker, rx_worker) = mpsc::channel(64);
        let worker = EndpointWorker {
            endpoint_id: endpoint_id.to_string(),
            url: url.to_string(),
            max_tabs,
            tab_cooldown_ms,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: HashSet::new(),
            cache: HashMap::new(),
        };
        tokio::spawn(run_worker(worker, rx_worker));
        workers.insert(endpoint_id.to_string(), tx_worker.clone());
        tx_worker
    };

    let req = LlmRequest {
        req_id,
        node_id: node_id.map(|v| v.to_string()),
        cache_key,
        prompt: prompt.to_string(),
        role_schema: role_schema.to_string(),
        phase: phase.to_string(),
        response: tx,
    };

    sender
        .send(req)
        .await
        .map_err(|_| anyhow::anyhow!("endpoint worker closed"))?;

    rx.await.map_err(|_| anyhow::anyhow!("endpoint worker canceled"))?
}

pub async fn init_workers(
    bridge: &WsBridge,
    config: &CapabilityConfig,
    tabs: &TabsHandle,
) {
    let mut workers = WORKERS.lock().await;
    for endpoint in &config.llm_endpoints {
        if workers.contains_key(&endpoint.id) {
            continue;
        }
        let (tx_worker, rx_worker) = mpsc::channel(64);
        let worker = EndpointWorker {
            endpoint_id: endpoint.id.clone(),
            url: endpoint.url.clone(),
            max_tabs: endpoint.max_tabs,
            tab_cooldown_ms: config.tab_cooldown_ms,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: HashSet::new(),
            cache: HashMap::new(),
        };
        tokio::spawn(run_worker(worker, rx_worker));
        workers.insert(endpoint.id.clone(), tx_worker);
    }
}

async fn run_worker(mut worker: EndpointWorker, mut rx: mpsc::Receiver<LlmRequest>) {
    while let Some(req) = rx.recv().await {
        worker.handle_request(req).await;
    }
}

fn stable_hash64(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn response_matches_req_id(raw: &str, req_id: u64) -> bool {
    let needle = format!("[REQ_ID:{}]", req_id);
    raw.contains(&needle)
}
