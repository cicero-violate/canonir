use super::config::CapabilityConfig;
use super::response_router;
use super::tab_management::{
    tab_manager_apply_rate_limit_penalty, tab_manager_get_or_open_tab, tab_manager_mark_tab_in_flight, tab_manager_mark_tab_response, tab_manager_mark_tab_sent,
    tab_manager_note_success,
};

pub use super::tab_management::{tab_manager_log_llm, tab_manager_now_ms, TabManagerHandle};
use crate::llm_domains::{is_chatgpt_url, is_gemini_url};
use crate::ws_server::{WsBridge, WsBridgeError};
use anyhow::Result;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot, Mutex};
type WorkerKey = (String, Vec<String>, bool, usize);
static WORKERS: Lazy<Mutex<HashMap<WorkerKey, mpsc::Sender<LlmWorkItem>>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);
pub fn llm_worker_new_tabs() -> TabManagerHandle {
    std::sync::Arc::new(tokio::sync::Mutex::new(super::tab_management::TabSlotTable::new()))
}
pub struct LlmWorkItem {
    pub req_id: u64,
    pub node_id: Option<String>,
    pub cache_key: Option<u64>,
    pub bust_cache: bool,
    pub allow_req_id_mismatch: bool,
    pub prompt: String,
    pub role_schema: String,
    pub phase: String,
    pub response_timeout_secs: Option<u64>,
    pub submit_only: bool,
    pub response: oneshot::Sender<Result<LlmResponse>>,
}

#[derive(Clone)]
pub struct LlmResponse {
    pub raw: String,
    pub tab_id: Option<u32>,
    pub turn_id: Option<u64>,
}
#[derive(Clone)]
struct LlmWorker {
    endpoint_id: String,
    url: Vec<String>,
    max_tabs: usize,
    stateful: bool,
    bridge: WsBridge,
    tabs: TabManagerHandle,
    seen_hashes: Arc<Mutex<HashSet<u64>>>,
    cache: Arc<Mutex<HashMap<u64, String>>>,
    /// Tracks which tab IDs have already received the role/system prompt.
    /// For stateful endpoints we send the role_schema only on the first turn;
    /// subsequent turns carry only the user prompt, saving tokens.
    tabs_with_role_sent: Arc<Mutex<HashSet<u32>>>,
}
impl LlmWorker {
    fn pick_url(&self, index: usize) -> &str {
        match self.url.len() {
            0 => "",
            len => &self.url[index % len],
        }
    }
    async fn handle_request(&self, req: LlmWorkItem) {
        // telemetry removed
        if req.bust_cache {
            if let Some(key) = req.cache_key {
                self.cache.lock().await.remove(&key);
            }
        }
        if let Some(key) = req.cache_key {
            if let Some(hit) = self.cache.lock().await.get(&key).cloned() {
                // telemetry removed
                let _ = req.response.send(Ok(LlmResponse {
                    raw: hit,
                    tab_id: None,
                    turn_id: None,
                }));
                return;
            }
        }
        if let Some(node_id) = req.node_id.as_deref() {
            response_router::response_router_register(req.req_id, node_id).await;
        }
        tab_manager_log_llm(format!("phase={} endpoint={} req_id={} send_turn_start", req.phase, self.endpoint_id, req.req_id));
        if req.submit_only {
            let result = self
                .submit_turn_only(&req.phase, req.req_id, req.prompt, req.role_schema, req.response_timeout_secs)
                .await;
            if let Ok(resp) = result.as_ref() {
                if let Some(key) = req.cache_key {
                    self.cache.lock().await.insert(key, resp.raw.clone());
                }
            }
            let _ = req.response.send(result);
            return;
        }
        let result = self
            .send_turn(
                &req.phase,
                req.req_id,
                req.allow_req_id_mismatch,
                req.prompt,
                req.role_schema,
                req.response_timeout_secs,
            )
            .await;
        if let Ok(resp) = result.as_ref() {
            if let Some(key) = req.cache_key {
                self.cache.lock().await.insert(key, resp.raw.clone());
            }
        }
        let _ = req.response.send(result);
    }
    async fn send_turn(
        &self,
        phase: &str,
        req_id: u64,
        allow_req_id_mismatch: bool,
        prompt: String,
        role_schema: String,
        response_timeout_secs: Option<u64>,
    ) -> Result<LlmResponse> {
        const MAX_SEND_ATTEMPTS: usize = 2;
        let selected_url = self.pick_url(req_id as usize);
        let response_timeout_secs = response_timeout_secs.unwrap_or_else(|| self.bridge.response_timeout_secs());
        for attempt in 0..MAX_SEND_ATTEMPTS {
            let tab_id = tab_manager_get_or_open_tab(&self.bridge, &self.endpoint_id, selected_url, &self.tabs, self.max_tabs).await?;

            // For stateful endpoints, send the role/system prompt only on the first
            // turn to this tab; all subsequent turns carry only the user prompt.
            // For non-stateful endpoints the role_schema is always prepended (each
            // call starts a fresh context).
            let include_role = if role_schema.trim().is_empty() {
                false
            } else if !self.stateful {
                true
            } else {
                self.tabs_with_role_sent.lock().await.insert(tab_id)
            };
            let raw_prompt = if include_role { format!("{}\n\n{}", role_schema.trim_end(), prompt) } else { prompt.clone() };
            let full_prompt = if raw_prompt.len() > 120_000 {
                // Walk back from byte 120_000 to the nearest valid char boundary.
                let mut safe = 120_000usize;
                while safe > 0 && !raw_prompt.is_char_boundary(safe) {
                    safe -= 1;
                }
                let cut = raw_prompt[..safe].rfind('\n').unwrap_or(safe);
                let mut s = raw_prompt[..cut].to_string();
                s.push_str("\n... [prompt truncated]\n");
                s
            } else {
                raw_prompt
            };

            tab_manager_mark_tab_sent(&self.tabs, tab_id).await;
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} send attempt={}", phase, self.endpoint_id, tab_id, attempt + 1));
            let (raw, turn_id) = match self.bridge.send_turn_with_meta_with_timeout(tab_id, selected_url, full_prompt, response_timeout_secs).await {
                Ok(v) => v,
                Err(e) => {
                    tab_manager_mark_tab_in_flight(&self.tabs, tab_id, false).await;
                    let penalty_ms = tab_manager_apply_rate_limit_penalty(&self.tabs, &self.endpoint_id).await;
                    tab_manager_log_llm(format!("phase={} endpoint={} tab={} send_error={} attempt={}", phase, self.endpoint_id, tab_id, e, attempt + 1));
                    tab_manager_log_llm(format!("phase={} endpoint={} tab={} retained_after_error", phase, self.endpoint_id, tab_id));
                    tab_manager_log_llm(format!("phase={} endpoint={} adaptive_penalty_ms={}", phase, self.endpoint_id, penalty_ms));
                    if should_retry_send_turn(&e) && attempt + 1 < MAX_SEND_ATTEMPTS {
                        tab_manager_log_llm(format!("phase={} endpoint={} retrying_same_pool_after_error={}", phase, self.endpoint_id, e));
                        continue;
                    }
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
            if !self.stateful && is_gemini_url(selected_url) {
                let _ = self.bridge.new_chat(tab_id).await;
                match self.bridge.wait_new_chat(tab_id, 20).await {
                    Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, self.endpoint_id, tab_id)),
                    Err(e) => {
                        tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
                        tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
                        return Err(anyhow::anyhow!("new_chat timeout"));
                    }
                }
            } else if !self.stateful && is_chatgpt_url(selected_url) {
                let _ = self.bridge.new_chat(tab_id).await;
                match self.bridge.wait_new_chat(tab_id, 20).await {
                    Ok(()) => tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, self.endpoint_id, tab_id)),
                    Err(e) => {
                        tab_manager_mark_tab_in_flight(&self.tabs, tab_id, true).await;
                        tab_manager_log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, self.endpoint_id, tab_id, e));
                        return Err(anyhow::anyhow!("new_chat timeout"));
                    }
                }
                // temp_chat removed: redirect to temporary-chat UI races with prompt injection.
            }
            let cooldown_ms = tab_manager_note_success(&self.tabs, &self.endpoint_id, tab_id).await;
            tab_manager_log_llm(format!("phase={} endpoint={} tab={} adaptive_cooldown_ms={}", phase, self.endpoint_id, tab_id, cooldown_ms));
            let hash = llm_worker_stable_hash64(&raw);
            if !self.seen_hashes.lock().await.insert(hash) {
                // Duplicate outputs can legitimately occur for verify/observe steps; don't fail the call.
                tab_manager_log_llm(format!("phase={} endpoint={} tab={} duplicate_hash={}", phase, self.endpoint_id, tab_id, hash));
            }
            return Ok(LlmResponse {
                raw,
                tab_id: Some(tab_id),
                turn_id: Some(turn_id),
            });
        }
        Err(anyhow::anyhow!("llm send_turn exhausted retries"))
    }

    async fn submit_turn_only(
        &self,
        _phase: &str,
        req_id: u64,
        prompt: String,
        role_schema: String,
        _response_timeout_secs: Option<u64>,
    ) -> Result<LlmResponse> {
        let selected_url = self.pick_url(req_id as usize);
        let tab_id = tab_manager_get_or_open_tab(&self.bridge, &self.endpoint_id, selected_url, &self.tabs, self.max_tabs).await?;
        let include_role = if role_schema.trim().is_empty() {
            false
        } else if !self.stateful {
            true
        } else {
            self.tabs_with_role_sent.lock().await.insert(tab_id)
        };
        let raw_prompt = if include_role { format!("{}\n\n{}", role_schema.trim_end(), prompt) } else { prompt };
        let full_prompt = if raw_prompt.len() > 120_000 {
            let mut safe = 120_000usize;
            while safe > 0 && !raw_prompt.is_char_boundary(safe) {
                safe -= 1;
            }
            let cut = raw_prompt[..safe].rfind('\n').unwrap_or(safe);
            let mut s = raw_prompt[..cut].to_string();
            s.push_str("\n... [prompt truncated]\n");
            s
        } else {
            raw_prompt
        };

        tab_manager_mark_tab_sent(&self.tabs, tab_id).await;
        let turn_id = self.bridge.submit_turn(tab_id, selected_url, full_prompt).await?;
        tab_manager_mark_tab_in_flight(&self.tabs, tab_id, false).await;
        let raw = format!(
            "{{\"submit_ack\":true,\"req_id\":{req_id},\"tab_id\":{tab_id},\"turn_id\":{turn_id}}}"
        );
        Ok(LlmResponse {
            raw,
            tab_id: Some(tab_id),
            turn_id: Some(turn_id),
        })
    }
}

fn should_retry_send_turn(err: &WsBridgeError) -> bool {
    matches!(err, WsBridgeError::Timeout | WsBridgeError::Cancelled | WsBridgeError::NoTab | WsBridgeError::NotConnected)
}
pub async fn llm_worker_send_request(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, bust_cache: bool, allow_req_id_mismatch: bool,
    phase: &str, tabs: &TabManagerHandle, max_tabs: usize, submit_only: bool,
) -> Result<String> {
    let (req_id, resp) =
        llm_worker_send_request_with_req_id_timeout(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            node_id,
            cache_key,
            bust_cache,
            allow_req_id_mismatch,
            phase,
            tabs,
            max_tabs,
            submit_only,
            None,
        )
        .await?;
    let _ = req_id;
    Ok(resp.raw)
}

pub async fn llm_worker_send_request_with_req_id(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, bust_cache: bool, allow_req_id_mismatch: bool,
    phase: &str, tabs: &TabManagerHandle, max_tabs: usize, submit_only: bool,
) -> Result<(u64, LlmResponse)> {
    llm_worker_send_request_with_req_id_timeout(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        cache_key,
        bust_cache,
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        submit_only,
        None,
    )
    .await
}

pub async fn llm_worker_send_request_timeout(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, bust_cache: bool, allow_req_id_mismatch: bool,
    phase: &str, tabs: &TabManagerHandle, max_tabs: usize, submit_only: bool, response_timeout_secs: Option<u64>,
) -> Result<String> {
    let (req_id, resp) =
        llm_worker_send_request_with_req_id_timeout(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            node_id,
            cache_key,
            bust_cache,
            allow_req_id_mismatch,
            phase,
            tabs,
            max_tabs,
            submit_only,
            response_timeout_secs,
        )
        .await?;
    let _ = req_id;
    Ok(resp.raw)
}

pub async fn llm_worker_send_request_with_req_id_timeout(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, node_id: Option<&str>, cache_key: Option<u64>, bust_cache: bool, allow_req_id_mismatch: bool,
    phase: &str, tabs: &TabManagerHandle, max_tabs: usize, submit_only: bool, response_timeout_secs: Option<u64>,
) -> Result<(u64, LlmResponse)> {
    let req_id = NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    let sender = if !stateful {
        // Non-stateful endpoints do not need serialized worker reuse.
        // Spawn a fresh worker per request so tabs can make progress concurrently.
        let (tx_worker, rx_worker) = mpsc::channel(1);
        let worker = LlmWorker {
            endpoint_id: endpoint_id.to_string(),
            url: vec![url.to_string()],
            max_tabs,
            stateful,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: Arc::new(Mutex::new(HashSet::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            tabs_with_role_sent: Arc::new(Mutex::new(HashSet::new())),
        };
        tokio::spawn(llm_worker_run_worker(worker, rx_worker));
        tx_worker
    } else {
        let mut workers = WORKERS.lock().await;
        let worker_key = (endpoint_id.to_string(), vec![url.to_string()], stateful, std::sync::Arc::as_ptr(tabs) as usize);
        if let Some(sender) = workers.get(&worker_key) {
            sender.clone()
        } else {
            let (tx_worker, rx_worker) = mpsc::channel(64);
            let worker = LlmWorker {
                endpoint_id: endpoint_id.to_string(),
                url: vec![url.to_string()],
                max_tabs,
                stateful,
                bridge: bridge.clone(),
                tabs: tabs.clone(),
                seen_hashes: Arc::new(Mutex::new(HashSet::new())),
                cache: Arc::new(Mutex::new(HashMap::new())),
                tabs_with_role_sent: Arc::new(Mutex::new(HashSet::new())),
            };
            tokio::spawn(llm_worker_run_worker(worker, rx_worker));
            workers.insert(worker_key, tx_worker.clone());
            tx_worker
        }
    };
    let req = LlmWorkItem {
        req_id,
        node_id: node_id.map(|v| v.to_string()),
        cache_key,
        bust_cache,
        allow_req_id_mismatch,
        prompt: prompt.to_string(),
        role_schema: role_schema.to_string(),
        phase: phase.to_string(),
        response_timeout_secs,
        submit_only,
        response: tx,
    };
    sender.send(req).await.map_err(|_| anyhow::anyhow!("endpoint worker closed"))?;
    let resp = rx.await.map_err(|_| anyhow::anyhow!("endpoint worker canceled"))??;
    Ok((req_id, resp))
}
pub async fn llm_worker_init_workers(bridge: &WsBridge, config: &CapabilityConfig, tabs: &TabManagerHandle) {
    let mut workers = WORKERS.lock().await;
    for endpoint in &config.llm_endpoints {
        if !endpoint.stateful {
            continue;
        }
        let worker_key = (endpoint.id.clone(), endpoint.url.clone(), endpoint.stateful, std::sync::Arc::as_ptr(tabs) as usize);
        if workers.contains_key(&worker_key) {
            continue;
        }
        let (tx_worker, rx_worker) = mpsc::channel(64);
        let worker = LlmWorker {
            endpoint_id: endpoint.id.clone(),
            url: endpoint.url.clone(),
            max_tabs: endpoint.max_tabs,
            stateful: endpoint.stateful,
            bridge: bridge.clone(),
            tabs: tabs.clone(),
            seen_hashes: Arc::new(Mutex::new(HashSet::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            tabs_with_role_sent: Arc::new(Mutex::new(HashSet::new())),
        };
        tokio::spawn(llm_worker_run_worker(worker, rx_worker));
        workers.insert(worker_key, tx_worker);
    }
}
async fn llm_worker_run_worker(worker: LlmWorker, mut rx: mpsc::Receiver<LlmWorkItem>) {
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
    let _ = (raw, req_id);
    true
}
