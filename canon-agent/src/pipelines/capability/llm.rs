use anyhow::{Context, Result};
use serde_json::Value;
use std::hash::{Hash, Hasher};

use crate::llm_provider::JsonExtractor;
use crate::ws_server::WsBridge;
use super::endpoint_worker::{self, TabsHandle, log_llm, now_ms};

pub async fn call_agent_json(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
) -> Result<Value> {
    call_agent_json_inner(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        false,
    )
    .await
}

async fn call_agent_json_inner(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    allow_req_id_mismatch: bool,
) -> Result<Value> {
    let cache_key = cache_key_for(prompt, role_schema);
    let raw = endpoint_worker::send_request(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        Some(cache_key),
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let log_dir = "/workspace/ai_sandbox/canon/agent_logs/capability/llm_raw";
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
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    call_agent_json_with_retry_inner(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        max_retries,
        delay_secs,
        false,
    )
    .await
}

pub async fn call_agent_json_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    call_agent_json_with_retry_inner(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        max_retries,
        delay_secs,
        true,
    )
    .await
}

pub async fn call_agent_raw_with_retry_allow_mismatch(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<String> {
    call_agent_raw_with_retry_inner(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        phase,
        node_id,
        tabs,
        max_tabs,
        tab_cooldown_ms,
        max_retries,
        delay_secs,
        true,
    )
    .await
}

async fn call_agent_json_with_retry_inner(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
    allow_req_id_mismatch: bool,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = now_ms();
        log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match call_agent_json_inner(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            phase,
            node_id,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            allow_req_id_mismatch,
        )
        .await {
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

async fn call_agent_raw_with_retry_inner(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
    allow_req_id_mismatch: bool,
) -> Result<String> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = now_ms();
        log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match call_agent_raw_inner(
            bridge,
            endpoint_id,
            url,
            stateful,
            prompt,
            role_schema,
            phase,
            node_id,
            tabs,
            max_tabs,
            tab_cooldown_ms,
            allow_req_id_mismatch,
        )
        .await {
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

async fn call_agent_raw_inner(
    bridge: &WsBridge,
    endpoint_id: &str,
    url: &str,
    stateful: bool,
    prompt: &str,
    role_schema: &str,
    phase: &str,
    node_id: Option<&str>,
    tabs: &TabsHandle,
    max_tabs: usize,
    tab_cooldown_ms: u64,
    allow_req_id_mismatch: bool,
) -> Result<String> {
    let cache_key = cache_key_for(prompt, role_schema);
    let raw = endpoint_worker::send_request(
        bridge,
        endpoint_id,
        url,
        stateful,
        prompt,
        role_schema,
        node_id,
        Some(cache_key),
        allow_req_id_mismatch,
        phase,
        tabs,
        max_tabs,
        tab_cooldown_ms,
    )
    .await?;
    let log_dir = "/workspace/ai_sandbox/canon/agent_logs/capability/llm_raw";
    let _ = std::fs::create_dir_all(log_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let raw_path = format!("{}/llm_raw_full_{}_{}.txt", log_dir, endpoint_id, ts);
    let _ = std::fs::write(&raw_path, &raw);
    Ok(raw)
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

fn cache_key_for(prompt: &str, role_schema: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut hasher);
    role_schema.hash(&mut hasher);
    hasher.finish()
}
