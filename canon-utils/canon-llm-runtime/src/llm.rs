use super::endpoint_worker::{self, tab_manager_log_llm, tab_manager_now_ms, TabManagerHandle};
use crate::ws_server::WsBridge;

/// Normalize raw LLM output into a structured `Value`.
///
/// Steps:
/// 1. Fix escape sequences (`\n`, `\"`)
/// 2. Strip markdown fences (` ```json ... ``` `)
/// 3. Parse as JSON → return `Value` on success
/// 4. Fallback: log `LLM_NORMALIZE_FALLBACK` and return `{"text": <raw>}`
pub fn normalize_llm_output(raw: &str) -> Value {
    // 1) Try the raw string first — handles properly escaped JSON directly.
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return v;
    }
    if let Some(v) = try_parse_lenient_json(trimmed) {
        return v;
    }

    // 2) Strip markdown fence without touching escapes.
    let fenced = strip_json_fence(trimmed).trim();
    if let Ok(v) = serde_json::from_str::<Value>(fenced) {
        return v;
    }

    // 3) Apply escape-heuristic only as a last resort for doubly-escaped payloads.
    let s = raw.replace("\\n", "\n").replace("\\\"", "\"");
    let cleaned = strip_json_fence(s.trim()).trim();
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        return v;
    }
    if let Some(v) = try_parse_lenient_json(cleaned) {
        return v;
    }
    serde_json::json!({ "text": raw })
}

fn strip_json_fence(s: &str) -> &str {
    let trimmed = s.trim_start();
    let mut tick_count = 0usize;
    for ch in trimmed.chars() {
        if ch == '`' {
            tick_count += 1;
        } else {
            break;
        }
    }
    if tick_count >= 3 {
        let fence = "`".repeat(tick_count);
        let mut inner = trimmed.strip_prefix(&fence).unwrap_or(trimmed).trim_start();
        if inner.starts_with("json") || inner.starts_with("JSON") {
            inner = inner[4..].trim_start_matches(['\n', '\r', ' ']).trim_start();
        }
        if let Some(close) = inner.rfind(&fence) {
            return inner[..close].trim();
        }
    }
    for prefix in ["```json\n", "```json\r\n", "```json ", "```json", "```JSON\n", "```JSON\r\n", "```JSON ", "```JSON", "```\n", "```\r\n", "```"] {
        if let Some(inner) = s.strip_prefix(prefix) {
            if let Some(close) = inner.rfind("```") {
                return inner[..close].trim();
            }
        }
    }
    s
}

use anyhow::Result;
use serde_json::Value;
use std::hash::{Hash, Hasher};
pub async fn request_agent_json(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64,
) -> Result<Value> {
    llm_client_call_agent_json_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, false, false).await
}
async fn llm_client_call_agent_json_inner(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, allow_req_id_mismatch: bool, bust_cache: bool,
) -> Result<Value> {
    let cache_key = llm_client_cache_key_for(prompt, role_schema);
    let raw = endpoint_worker::llm_worker_send_request(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        Some(cache_key),
        bust_cache,
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let log_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm_raw";
    let _ = std::fs::create_dir_all(log_dir);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let raw_path = format!("{}/llm_raw_full_{}_{}.txt", log_dir, endpoint_id, ts);
    let _ = std::fs::write(&raw_path, &raw);
    Ok(normalize_llm_output(&raw))
}
async fn llm_client_call_agent_json_inner_with_req_id(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, allow_req_id_mismatch: bool, bust_cache: bool,
) -> Result<(Value, u64)> {
    let cache_key = llm_client_cache_key_for(prompt, role_schema);
    let (req_id, raw) = endpoint_worker::llm_worker_send_request_with_req_id(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        Some(cache_key),
        bust_cache,
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let log_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm_raw";
    let _ = std::fs::create_dir_all(log_dir);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let raw_path = format!("{}/llm_raw_full_{}_{}.txt", log_dir, endpoint_id, ts);
    let _ = std::fs::write(&raw_path, &raw);
    Ok((normalize_llm_output(&raw), req_id))
}
pub async fn llm_client_call_agent_json_with_retry(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64,
) -> Result<Value> {
    llm_client_call_agent_json_with_retry_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, max_retries, delay_secs, false, false).await
}
pub async fn llm_client_call_agent_json_with_retry_allow_mismatch(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64, bust_cache: bool,
) -> Result<Value> {
    llm_client_call_agent_json_with_retry_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, max_retries, delay_secs, true, bust_cache)
        .await
}
pub async fn llm_client_call_agent_json_with_retry_allow_mismatch_with_req_id(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64,
) -> Result<(Value, u64)> {
    llm_client_call_agent_json_with_retry_inner_with_req_id(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, max_retries, delay_secs, true)
        .await
}
pub async fn llm_client_call_agent_raw_with_retry_allow_mismatch(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64, bust_cache: bool,
) -> Result<Value> {
    llm_client_call_agent_raw_with_retry_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, max_retries, delay_secs, true, bust_cache)
        .await
}
async fn llm_client_call_agent_json_with_retry_inner(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64, allow_req_id_mismatch: bool, bust_cache: bool,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = tab_manager_now_ms();
        tab_manager_log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match llm_client_call_agent_json_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, allow_req_id_mismatch, bust_cache).await {
            Ok(v) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} ok elapsed_ms={}", phase, endpoint_id, attempt + 1, elapsed));
                return Ok(v);
            }
            Err(e) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} error={} elapsed_ms={}", phase, endpoint_id, attempt + 1, e, elapsed));
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("llm retries exhausted")))
}
async fn llm_client_call_agent_json_with_retry_inner_with_req_id(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64, allow_req_id_mismatch: bool,
) -> Result<(Value, u64)> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = tab_manager_now_ms();
        tab_manager_log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match llm_client_call_agent_json_inner_with_req_id(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, allow_req_id_mismatch, false).await
        {
            Ok(v) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} ok elapsed_ms={}", phase, endpoint_id, attempt + 1, elapsed));
                return Ok(v);
            }
            Err(e) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} error={} elapsed_ms={}", phase, endpoint_id, attempt + 1, e, elapsed));
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("llm retries exhausted")))
}
async fn llm_client_call_agent_raw_with_retry_inner(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, max_retries: u32, delay_secs: u64, allow_req_id_mismatch: bool, bust_cache: bool,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = tab_manager_now_ms();
        tab_manager_log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match llm_client_call_agent_raw_inner(bridge, endpoint_id, url, stateful, prompt, role_schema, phase, node_id, tabs, max_tabs, tab_cooldown_ms, allow_req_id_mismatch, bust_cache).await {
            Ok(v) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} ok elapsed_ms={}", phase, endpoint_id, attempt + 1, elapsed));
                return Ok(v);
            }
            Err(e) => {
                let elapsed = tab_manager_now_ms().saturating_sub(start);
                tab_manager_log_llm(format!("phase={} endpoint={} attempt={} error={} elapsed_ms={}", phase, endpoint_id, attempt + 1, e, elapsed));
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("llm retries exhausted")))
}
async fn llm_client_call_agent_raw_inner(
    bridge: &WsBridge, endpoint_id: &str, url: &str, stateful: bool, prompt: &str, role_schema: &str, phase: &str, node_id: Option<&str>, tabs: &TabManagerHandle, max_tabs: usize,
    tab_cooldown_ms: u64, allow_req_id_mismatch: bool, bust_cache: bool,
) -> Result<Value> {
    let cache_key = llm_client_cache_key_for(prompt, role_schema);
    let raw = endpoint_worker::llm_worker_send_request(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        Some(cache_key),
        bust_cache,
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let log_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm_raw";
    let _ = std::fs::create_dir_all(log_dir);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let raw_path = format!("{}/llm_raw_full_{}_{}.txt", log_dir, endpoint_id, ts);
    let _ = std::fs::write(&raw_path, &raw);
    Ok(normalize_llm_output(&raw))
}
fn try_parse_lenient_json(raw: &str) -> Option<Value> {
    let start = raw.find('{').or_else(|| raw.find('['))?;
    let end = raw.rfind('}').or_else(|| raw.rfind(']'))?;
    if end <= start {
        return None;
    }
    let slice = raw[start..=end].trim();
    serde_json::from_str(slice).ok()
}
fn llm_client_cache_key_for(prompt: &str, role_schema: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut hasher);
    role_schema.hash(&mut hasher);
    hasher.finish()
}
