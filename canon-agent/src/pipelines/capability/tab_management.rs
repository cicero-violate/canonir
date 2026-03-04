use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use crate::ws_server::WsBridge;
use super::console;

pub struct TabSlots {
    pub owner: HashMap<String, u32>,
    pub meta: HashMap<u32, TabMeta>,
}

impl TabSlots {
    pub fn new() -> Self {
        Self { owner: HashMap::new(), meta: HashMap::new() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TabMeta {
    pub last_sent_ms: Option<u128>,
    pub last_response_ms: Option<u128>,
    pub in_flight: bool,
    pub cooldown_until_ms: Option<u128>,
}

pub type TabsHandle = Arc<tokio::sync::Mutex<TabSlots>>;

pub async fn get_or_open_tab(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    tabs: &TabsHandle,
    _max_tabs: usize,
) -> Result<u32> {
    if let Some(id) = get_owner_tab(endpoint_id, tabs).await {
        log_llm(format!("endpoint={} reuse owner_tab={}", endpoint_id, id));
        return Ok(id);
    }
    bridge.wait_for_connection().await;
    log_llm(format!("endpoint={} opening_new_tab url={}", endpoint_id, url));
    let open = bridge.open_fresh_tab_with_url(url.to_string());
    let id = match tokio::time::timeout(std::time::Duration::from_secs(20), open).await {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => return Err(anyhow::anyhow!("failed to open tab: {e}")),
        Err(_) => {
            log_llm(format!("endpoint={} open_tab_timeout", endpoint_id));
            return Err(anyhow::anyhow!("open tab timeout"));
        }
    };
    set_tab_id(endpoint_id, id, tabs, _max_tabs).await;
    mark_tab_in_flight(tabs, id, true).await;
    log_llm(format!("endpoint={} opened tab={}", endpoint_id, id));
    Ok(id)
}

pub async fn get_owner_tab(endpoint_id: &str, tabs: &TabsHandle) -> Option<u32> {
    let mut tabs = tabs.lock().await;
    let id = tabs.owner.get(endpoint_id).copied()?;
    let meta = tabs.meta.entry(id).or_default();
    meta.in_flight = true;
    Some(id)
}

pub async fn set_tab_id(endpoint_id: &str, id: u32, tabs: &TabsHandle, _max_tabs: usize) {
    let mut tabs = tabs.lock().await;
    tabs.owner.insert(endpoint_id.to_string(), id);
    tabs.meta.entry(id).or_default();
}

pub async fn mark_tab_sent(tabs: &TabsHandle, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_sent_ms = Some(now_ms());
}

pub async fn mark_tab_response(tabs: &TabsHandle, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_response_ms = Some(now_ms());
}

pub async fn mark_tab_in_flight(tabs: &TabsHandle, id: u32, in_flight: bool) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.in_flight = in_flight;
}

pub async fn mark_tab_cooldown(tabs: &TabsHandle, id: u32, cooldown_ms: u64) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.cooldown_until_ms = Some(now_ms().saturating_add(cooldown_ms as u128));
}

pub async fn drop_tab(tabs: &TabsHandle, endpoint_id: &str, id: u32) {
    let mut tabs = tabs.lock().await;
    if let Some(current) = tabs.owner.get(endpoint_id).copied() {
        if current == id {
            tabs.owner.remove(endpoint_id);
        }
    }
    tabs.meta.remove(&id);
    log_llm(format!("endpoint={} tab={} dropped", endpoint_id, id));
}

pub async fn summarize_tab_state(endpoint_id: &str, tabs: &TabsHandle) -> Option<String> {
    let tabs = tabs.lock().await;
    let id = tabs.owner.get(endpoint_id).copied()?;
    let meta = tabs.meta.get(&id);
    let in_flight = meta.map(|m| m.in_flight).unwrap_or(false);
    let last_resp = meta.and_then(|m| m.last_response_ms).unwrap_or(0);
    let cooldown = meta.and_then(|m| m.cooldown_until_ms).unwrap_or(0);
    Some(format!("tab={} in_flight={} last_resp_ms={} cooldown_until_ms={}", id, in_flight, last_resp, cooldown))
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn log_llm(message: String) {
    eprintln!("{}", console::llm(&format!("{} {}", now_ms(), message)));
}
