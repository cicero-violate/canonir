use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;

use crate::llm_provider::JsonExtractor;
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
}

pub async fn call_agent_json(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    tabs: &tokio::sync::Mutex<TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
) -> Result<Value> {
    let tab_id = get_or_open_tab(bridge, endpoint_id, url, tabs, reuse_tabs, max_tabs).await?;
    mark_tab_sent(tabs, tab_id).await;
    log_llm(format!("phase={} endpoint={} tab={} send", phase, endpoint_id, tab_id));
    let full_prompt = if role_schema.trim().is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n\n{}", role_schema.trim_end(), prompt)
    };
    let raw = match bridge.send_turn(tab_id, full_prompt).await {
        Ok(v) => v,
        Err(e) => {
            mark_tab_in_flight(tabs, tab_id, false).await;
            drop_tab(tabs, endpoint_id, tab_id).await;
            log_llm(format!("phase={} endpoint={} tab={} send_error={}", phase, endpoint_id, tab_id, e));
            if !reuse_tabs {
                let _ = bridge.close_tab(tab_id).await;
            }
            return Err(anyhow::anyhow!("llm send_turn error: {e}"));
        }
    };
    mark_tab_response(tabs, tab_id).await;
    mark_tab_in_flight(tabs, tab_id, false).await;
    log_llm(format!("phase={} endpoint={} tab={} response_ok bytes={}", phase, endpoint_id, tab_id, raw.len()));
    if reuse_tabs {
        if url.starts_with("https://gemini.google.com/") {
            let _ = bridge.new_chat(tab_id).await;
            log_llm(format!("phase={} endpoint={} tab={} new_chat", phase, endpoint_id, tab_id));
        } else if url.starts_with("https://chatgpt.com/") {
            let _ = bridge.temp_chat(tab_id).await;
            let _ = bridge.new_chat(tab_id).await;
            log_llm(format!("phase={} endpoint={} tab={} temp_chat+new_chat", phase, endpoint_id, tab_id));
        }
    } else {
        let _ = bridge.close_tab(tab_id).await;
    }
    let log_dir = "/workspace/ai_sandbox/canon/agent_logs/capability";
    let _ = std::fs::create_dir_all(log_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let raw_path = format!("{}/llm_raw_full_{}_{}.txt", log_dir, endpoint_id, ts);
    let _ = std::fs::write(&raw_path, &raw);

    let payload = match JsonExtractor::extract(&raw) {
        Ok(v) => v,
        Err(_) => {
            // Fallback: try to parse the first {...} or [...] region
            if let Some(v) = try_parse_loose_json(&raw) {
                v
            } else {
                let path = format!("{}/llm_raw_{}_{}.txt", log_dir, endpoint_id, ts);
                let _ = std::fs::write(&path, &raw);
                return Err(anyhow::anyhow!("json extract error"));
            }
        }
    };
    Ok(payload)
}

pub async fn call_agent_json_with_retry(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    tabs: &tokio::sync::Mutex<TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = now_ms();
        log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match call_agent_json(bridge, endpoint_id, url, prompt, role_schema, phase, tabs, reuse_tabs, max_tabs).await {
            Ok(v) => {
                let elapsed = now_ms().saturating_sub(start);
                log_llm(format!("phase={} endpoint={} attempt={} ok elapsed_ms={}", phase, endpoint_id, attempt + 1, elapsed));
                return Ok(v);
            }
            Err(e) => {
                let elapsed = now_ms().saturating_sub(start);
                log_llm(format!("phase={} endpoint={} attempt={} error={} elapsed_ms={}", phase, endpoint_id, attempt + 1, e, elapsed));
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("llm retries exhausted")))
}

async fn get_or_open_tab(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    tabs: &tokio::sync::Mutex<TabSlots>,
    reuse_tabs: bool,
    max_tabs: usize,
) -> Result<u32> {
    let slots_len = {
        let tabs = tabs.lock().await;
        tabs.slots.get(endpoint_id).map(|v| v.len()).unwrap_or(0)
    };
    if reuse_tabs {
        if let Some(id) = reserve_tab(endpoint_id, tabs).await {
            log_llm(format!("endpoint={} reuse tab={}", endpoint_id, id));
            return Ok(id);
        }
        // If reusing tabs and we already have some, do not open new ones.
        if slots_len > 0 {
            if let Some(id) = wait_for_available_tab(endpoint_id, tabs).await {
                log_llm(format!("endpoint={} waited tab={}", endpoint_id, id));
                return Ok(id);
            }
            return Err(anyhow::anyhow!("no available tab (reuse_tabs=true, slots_len={})", slots_len));
        }
    } else {
        let limit = if max_tabs == 0 { usize::MAX } else { max_tabs };
        if slots_len >= limit {
            if let Some(id) = reserve_tab(endpoint_id, tabs).await {
                log_llm(format!("endpoint={} reuse tab={}", endpoint_id, id));
                return Ok(id);
            }
        }
    }
    if reuse_tabs || (max_tabs != 0 && slots_len >= max_tabs) {
        if let Some(id) = wait_for_available_tab(endpoint_id, tabs).await {
            log_llm(format!("endpoint={} waited tab={}", endpoint_id, id));
            return Ok(id);
        }
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
    set_tab_id(endpoint_id, id, tabs, reuse_tabs, max_tabs).await;
    mark_tab_in_flight(tabs, id, true).await;
    log_llm(format!("endpoint={} opened tab={}", endpoint_id, id));
    Ok(id)
}

async fn reserve_tab(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<u32> {
    let mut tabs = tabs.lock().await;
    let slots_len = tabs.slots.get(endpoint_id).map(|v| v.len())?;
    if slots_len == 0 {
        return None;
    }
    let ids = tabs.slots.get(endpoint_id)?;
    let mut best_id = ids[0];
    let mut best_key = (true, u128::MAX);
    for id in ids {
        let meta = tabs.meta.get(id);
        if meta.map(|m| m.in_flight).unwrap_or(false) {
            continue;
        }
        let last = meta.and_then(|m| m.last_response_ms);
        let key = (last.is_some(), last.unwrap_or(0));
        if key < best_key {
            best_key = key;
            best_id = *id;
        }
    }
    let meta = tabs.meta.entry(best_id).or_default();
    meta.in_flight = true;
    Some(best_id)
}

async fn set_tab_id(endpoint_id: &str, id: u32, tabs: &tokio::sync::Mutex<TabSlots>, reuse_tabs: bool, max_tabs: usize) {
    let mut tabs = tabs.lock().await;
    let entry = tabs.slots.entry(endpoint_id.to_string()).or_default();
    if reuse_tabs {
        if entry.is_empty() {
            entry.push(id);
        }
        tabs.meta.entry(id).or_default();
        return;
    }
    if max_tabs == 0 || entry.len() < max_tabs {
        entry.push(id);
        tabs.meta.entry(id).or_default();
    } else if entry.is_empty() {
        entry.push(id);
        tabs.meta.entry(id).or_default();
    }
}

async fn mark_tab_sent(tabs: &tokio::sync::Mutex<TabSlots>, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_sent_ms = Some(now_ms());
}

async fn mark_tab_response(tabs: &tokio::sync::Mutex<TabSlots>, id: u32) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.last_response_ms = Some(now_ms());
}

async fn mark_tab_in_flight(tabs: &tokio::sync::Mutex<TabSlots>, id: u32, in_flight: bool) {
    let mut tabs = tabs.lock().await;
    let meta = tabs.meta.entry(id).or_default();
    meta.in_flight = in_flight;
}

async fn drop_tab(tabs: &tokio::sync::Mutex<TabSlots>, endpoint_id: &str, id: u32) {
    let mut tabs = tabs.lock().await;
    if let Some(entry) = tabs.slots.get_mut(endpoint_id) {
        entry.retain(|v| *v != id);
    }
    tabs.meta.remove(&id);
    log_llm(format!("endpoint={} tab={} dropped", endpoint_id, id));
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn log_llm(message: String) {
    eprintln!("[llm {}] {}", now_ms(), message);
}

async fn wait_for_available_tab(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<u32> {
    for _ in 0..100 {
        if let Some(id) = reserve_tab(endpoint_id, tabs).await {
            return Some(id);
        }
        if let Some(summary) = summarize_tab_state(endpoint_id, tabs).await {
            log_llm(format!("endpoint={} waiting_for_tab {}", endpoint_id, summary));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

async fn summarize_tab_state(endpoint_id: &str, tabs: &tokio::sync::Mutex<TabSlots>) -> Option<String> {
    let tabs = tabs.lock().await;
    let ids = tabs.slots.get(endpoint_id)?;
    let mut parts = Vec::new();
    for id in ids {
        let meta = tabs.meta.get(id);
        let in_flight = meta.map(|m| m.in_flight).unwrap_or(false);
        let last_resp = meta.and_then(|m| m.last_response_ms).unwrap_or(0);
        parts.push(format!("tab={} in_flight={} last_resp_ms={}", id, in_flight, last_resp));
    }
    Some(parts.join(" | "))
}

fn try_parse_loose_json(raw: &str) -> Option<Value> {
    let start = raw.find('{').or_else(|| raw.find('['))?;
    let end = raw.rfind('}').or_else(|| raw.rfind(']'))?;
    if end <= start {
        return None;
    }
    let slice = raw[start..=end].trim();
    serde_json::from_str(slice).ok()
}
