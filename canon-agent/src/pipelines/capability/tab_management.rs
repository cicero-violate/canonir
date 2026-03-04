use anyhow::Result;
use std::collections::HashMap;

use crate::ws_server::WsBridge;

pub struct TabSlots {
    pub slots: HashMap<String, Vec<u32>>,
    pub rr: HashMap<String, usize>,
    pub meta: HashMap<u32, TabMeta>,
}

impl TabSlots {
    pub fn new() -> Self {
        Self { slots: HashMap::new(), rr: HashMap::new(), meta: HashMap::new() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TabMeta {
    pub last_sent_ms: Option<u128>,
    pub last_response_ms: Option<u128>,
    pub in_flight: bool,
    pub cooldown_until_ms: Option<u128>,
}

pub async fn get_or_open_tab(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    tabs: &tokio::sync::Mutex<TabSlots>,
    max_tabs: usize,
) -> Result<u32> {
    let slots_len = {
        let tabs = tabs.lock().await;
        tabs.slots.get(endpoint_id).map(|v| v.len()).unwrap_or(0)
    };
    if let Some(id) = reserve_tab(endpoint_id, tabs).await {
        log_llm(format!("endpoint={} reuse tab={}", endpoint_id, id));
        return Ok(id);
    }
    let capacity = max_tabs == 0 || slots_len < max_tabs;
    if !capacity {
        if let Some(id) = wait_for_available_tab(endpoint_id, tabs).await {
            log_llm(format!("endpoint={} waited tab={}", endpoint_id, id));
            return Ok(id);
        }
        return Err(anyhow::anyhow!("no available tab (slots_len={})", slots_len));
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
    set_tab_id(endpoint_id, id, tabs, max_tabs).await;
    mark_tab_in_flight(tabs, id, true).await;
    log_llm(format!("endpoint={} opened tab={}", endpoint_id, id));
    Ok(id)
}

pub async fn reserve_tab(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<u32> {
    let mut tabs = tabs.lock().await;
    let slots_len = tabs.slots.get(endpoint_id).map(|v| v.len())?;
    if slots_len == 0 {
        return None;
    }
    let ids = tabs.slots.get(endpoint_id)?;
    let mut best_id: Option<u32> = None;
    let mut best_key = (true, u128::MAX);
    for id in ids {
        let meta = tabs.meta.get(id);
        if meta.map(|m| m.in_flight).unwrap_or(false) {
            continue;
        }
        if let Some(until) = meta.and_then(|m| m.cooldown_until_ms) {
            if now_ms() < until {
                continue;
            }
        }
        let last = meta.and_then(|m| m.last_response_ms);
        let key = (last.is_some(), last.unwrap_or(0));
        if key < best_key {
            best_key = key;
            best_id = Some(*id);
        }
    }
    let best_id = best_id?;
    let meta = tabs.meta.entry(best_id).or_default();
    meta.in_flight = true;
    Some(best_id)
}

pub async fn set_tab_id(endpoint_id: &str, id: u32, tabs: &tokio::sync::Mutex<TabSlots>, max_tabs: usize) {
    let mut tabs = tabs.lock().await;
    let entry = tabs.slots.entry(endpoint_id.to_string()).or_default();
    if max_tabs == 0 || entry.len() < max_tabs {
        if !entry.contains(&id) {
            entry.push(id);
        }
        tabs.meta.entry(id).or_default();
    }
}

pub async fn mark_tab_sent(tabs: &tokio::sync::Mutex<TabSlots>, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_sent_ms = Some(now_ms());
}

pub async fn mark_tab_response(tabs: &tokio::sync::Mutex<TabSlots>, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_response_ms = Some(now_ms());
}

pub async fn mark_tab_in_flight(tabs: &tokio::sync::Mutex<TabSlots>, id: u32, in_flight: bool) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.in_flight = in_flight;
}

pub async fn mark_tab_cooldown(tabs: &tokio::sync::Mutex<TabSlots>, id: u32, cooldown_ms: u64) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.cooldown_until_ms = Some(now_ms().saturating_add(cooldown_ms as u128));
}

pub async fn drop_tab(tabs: &tokio::sync::Mutex<TabSlots>, endpoint_id: &str, id: u32) {
    let mut tabs = tabs.lock().await;
    if let Some(entry) = tabs.slots.get_mut(endpoint_id) {
        entry.retain(|v| *v != id);
    }
    tabs.meta.remove(&id);
    log_llm(format!("endpoint={} tab={} dropped", endpoint_id, id));
}

pub async fn wait_for_available_tab(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<u32> {
    for _ in 0..100 {
        if let Some(id) = reserve_tab(endpoint_id, tabs).await {
            return Some(id);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

pub async fn summarize_tab_state(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<String> {
    let tabs = tabs.lock().await;
    let ids = tabs.slots.get(endpoint_id)?;
    let mut parts = Vec::new();
    for id in ids {
        let meta = tabs.meta.get(id);
        let in_flight = meta.map(|m| m.in_flight).unwrap_or(false);
        let last_resp = meta.and_then(|m| m.last_response_ms).unwrap_or(0);
        let cooldown = meta.and_then(|m| m.cooldown_until_ms).unwrap_or(0);
        parts.push(format!("tab={} in_flight={} last_resp_ms={} cooldown_until_ms={}", id, in_flight, last_resp, cooldown));
    }
    Some(parts.join(" | "))
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn log_llm(message: String) {
    eprintln!("[llm {}] {}", now_ms(), message);
}
