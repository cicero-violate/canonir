use crate::ws_server::WsBridge;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
pub struct TabSlotTable {
    pub owner: HashMap<String, u32>,
    pub meta: HashMap<u32, TabSlotMeta>,
    pub endpoint_backoff_ms: HashMap<String, u64>,
    pub endpoint_cooldown_until_ms: HashMap<String, u128>,
}
impl TabSlotTable {
    pub fn new() -> Self {
        Self {
            owner: HashMap::new(),
            meta: HashMap::new(),
            endpoint_backoff_ms: HashMap::new(),
            endpoint_cooldown_until_ms: HashMap::new(),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct TabSlotMeta {
    pub last_sent_ms: Option<u128>,
    pub last_response_ms: Option<u128>,
    pub in_flight: bool,
    pub cooldown_until_ms: Option<u128>,
}
const ADAPTIVE_MIN_COOLDOWN_MS: u64 = 1_000;
const ADAPTIVE_MAX_COOLDOWN_MS: u64 = 8_000;
const ADAPTIVE_SUCCESS_DECAY_MS: u64 = 250;
const ADAPTIVE_FAILURE_MULTIPLIER: u64 = 2;
pub type TabManagerHandle = Arc<tokio::sync::Mutex<TabSlotTable>>;
pub async fn tab_manager_get_or_open_tab(bridge: &WsBridge, endpoint_id: &str, url: &str, tabs: &TabManagerHandle, _max_tabs: usize) -> Result<u32> {
    tab_manager_wait_endpoint_cooldown(endpoint_id, tabs).await;
    if let Some(id) = tab_manager_get_owner_tab(endpoint_id, tabs).await {
        tab_manager_log_llm(format!("endpoint={} reuse owner_tab={}", endpoint_id, id));
        return Ok(id);
    }
    bridge.wait_for_connection().await;
    if let Some(id) = bridge.claim_tab_for_url(url).await {
        tab_manager_log_llm(format!("endpoint={} claimed_existing_tab={} url={}", endpoint_id, id, url));
        tab_manager_set_tab_id(endpoint_id, id, tabs, _max_tabs).await;
        tab_manager_mark_tab_in_flight(tabs, id, true).await;
        return Ok(id);
    }
    tab_manager_log_llm(format!("endpoint={} opening_new_tab url={}", endpoint_id, url));
    let open = bridge.open_fresh_tab_with_url(url.to_string());
    let id = match tokio::time::timeout(std::time::Duration::from_secs(20), open).await {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => return Err(anyhow::anyhow!("failed to open tab: {e}")),
        Err(_) => {
            tab_manager_log_llm(format!("endpoint={} open_tab_timeout", endpoint_id));
            return Err(anyhow::anyhow!("open tab timeout"));
        }
    };
    tab_manager_set_tab_id(endpoint_id, id, tabs, _max_tabs).await;
    tab_manager_mark_tab_in_flight(tabs, id, true).await;
    tab_manager_log_llm(format!("endpoint={} opened tab={}", endpoint_id, id));
    Ok(id)
}
async fn tab_manager_wait_endpoint_cooldown(endpoint_id: &str, tabs: &TabManagerHandle) {
    loop {
        let wait_ms = {
            let tabs = tabs.lock().await;
            let now = tab_manager_now_ms();
            let until = tabs.endpoint_cooldown_until_ms.get(endpoint_id).copied().unwrap_or(0);
            if until > now {
                Some(until.saturating_sub(now) as u64)
            } else {
                None
            }
        };
        match wait_ms {
            Some(delay_ms) => {
                tab_manager_log_llm(format!("endpoint={} adaptive_cooldown_wait_ms={}", endpoint_id, delay_ms));
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            None => return,
        }
    }
}
pub async fn tab_manager_get_owner_tab(endpoint_id: &str, tabs: &TabManagerHandle) -> Option<u32> {
    loop {
        let wait_ms = {
            let mut tabs = tabs.lock().await;
            let id = tabs.owner.get(endpoint_id).copied()?;
            let meta = tabs.meta.entry(id).or_default();
            let now = tab_manager_now_ms();
            let cooldown_until = meta.cooldown_until_ms.unwrap_or(0);
            if cooldown_until > now {
                Some((id, cooldown_until.saturating_sub(now) as u64))
            } else {
                meta.in_flight = true;
                return Some(id);
            }
        };
        let (id, delay_ms) = wait_ms?;
        tab_manager_log_llm(format!("endpoint={} tab={} cooldown_wait_ms={}", endpoint_id, id, delay_ms));
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
}
pub async fn tab_manager_set_tab_id(endpoint_id: &str, id: u32, tabs: &TabManagerHandle, _max_tabs: usize) {
    let mut tabs = tabs.lock().await;
    tabs.owner.insert(endpoint_id.to_string(), id);
    tabs.meta.entry(id).or_default();
}
pub async fn tab_manager_mark_tab_sent(tabs: &TabManagerHandle, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_sent_ms = Some(tab_manager_now_ms());
}
pub async fn tab_manager_mark_tab_response(tabs: &TabManagerHandle, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_response_ms = Some(tab_manager_now_ms());
}
pub async fn tab_manager_mark_tab_in_flight(tabs: &TabManagerHandle, id: u32, in_flight: bool) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.in_flight = in_flight;
}
pub async fn tab_manager_note_success(tabs: &TabManagerHandle, endpoint_id: &str, id: u32) -> u64 {
    let mut tabs = tabs.lock().await;
    let now = tab_manager_now_ms();
    let current = tabs.endpoint_backoff_ms.get(endpoint_id).copied().unwrap_or(ADAPTIVE_MIN_COOLDOWN_MS);
    let next = current.saturating_sub(ADAPTIVE_SUCCESS_DECAY_MS).max(ADAPTIVE_MIN_COOLDOWN_MS);
    tabs.endpoint_backoff_ms.insert(endpoint_id.to_string(), next);
    tabs.endpoint_cooldown_until_ms.insert(endpoint_id.to_string(), now.saturating_add(next as u128));
    let meta = tabs.meta.entry(id).or_default();
    meta.cooldown_until_ms = Some(now.saturating_add(next as u128));
    next
}
pub async fn tab_manager_apply_rate_limit_penalty(tabs: &TabManagerHandle, endpoint_id: &str) -> u64 {
    let mut tabs = tabs.lock().await;
    let now = tab_manager_now_ms();
    let current = tabs.endpoint_backoff_ms.get(endpoint_id).copied().unwrap_or(ADAPTIVE_MIN_COOLDOWN_MS);
    let grown = current.max(ADAPTIVE_MIN_COOLDOWN_MS).saturating_mul(ADAPTIVE_FAILURE_MULTIPLIER);
    let next = grown.clamp(ADAPTIVE_MIN_COOLDOWN_MS, ADAPTIVE_MAX_COOLDOWN_MS);
    tabs.endpoint_backoff_ms.insert(endpoint_id.to_string(), next);
    tabs.endpoint_cooldown_until_ms.insert(endpoint_id.to_string(), now.saturating_add(next as u128));
    next
}
pub async fn tab_manager_drop_tab(tabs: &TabManagerHandle, endpoint_id: &str, id: u32) {
    let mut tabs = tabs.lock().await;
    if let Some(current) = tabs.owner.get(endpoint_id).copied() {
        if current == id {
            tabs.owner.remove(endpoint_id);
        }
    }
    tabs.meta.remove(&id);
    tab_manager_log_llm(format!("endpoint={} tab={} dropped", endpoint_id, id));
}
pub async fn tab_manager_summarize_tab_state(endpoint_id: &str, tabs: &TabManagerHandle) -> Option<String> {
    let tabs = tabs.lock().await;
    let id = tabs.owner.get(endpoint_id).copied()?;
    let meta = tabs.meta.get(&id);
    let in_flight = meta.map(|m| m.in_flight).unwrap_or(false);
    let last_resp = meta.and_then(|m| m.last_response_ms).unwrap_or(0);
    let cooldown = meta.and_then(|m| m.cooldown_until_ms).unwrap_or(0);
    let endpoint_backoff = tabs.endpoint_backoff_ms.get(endpoint_id).copied().unwrap_or(ADAPTIVE_MIN_COOLDOWN_MS);
    let endpoint_cooldown = tabs.endpoint_cooldown_until_ms.get(endpoint_id).copied().unwrap_or(0);
    Some(format!(
        "tab={} in_flight={} last_resp_ms={} cooldown_until_ms={} endpoint_backoff_ms={} endpoint_cooldown_until_ms={}",
        id, in_flight, last_resp, cooldown, endpoint_backoff, endpoint_cooldown
    ))
}
pub fn tab_manager_now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}
pub fn tab_manager_log_llm(message: String) {
    let _ = message;
}
