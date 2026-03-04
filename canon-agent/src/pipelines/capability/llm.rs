use anyhow::{Context, Result};
use serde_json::Value;

use crate::llm_provider::JsonExtractor;
use crate::llm_domains::{is_chatgpt_url, is_gemini_url};
use crate::ws_server::WsBridge;
use super::tab_management::{
    TabSlots,
    get_or_open_tab,
    mark_tab_sent,
    mark_tab_response,
    mark_tab_in_flight,
    mark_tab_cooldown,
    drop_tab,
    log_llm,
    now_ms,
};

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
    tab_cooldown_ms: u64,
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
        if is_gemini_url(url) {
            let _ = bridge.new_chat(tab_id).await;
            match bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, endpoint_id, tab_id)),
                Err(e) => {
                    mark_tab_in_flight(tabs, tab_id, true).await;
                    log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
        } else if is_chatgpt_url(url) {
            let _ = bridge.new_chat(tab_id).await;
            match bridge.wait_new_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!("phase={} endpoint={} tab={} new_chat_done", phase, endpoint_id, tab_id)),
                Err(e) => {
                    mark_tab_in_flight(tabs, tab_id, true).await;
                    log_llm(format!("phase={} endpoint={} tab={} new_chat_timeout={}", phase, endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("new_chat timeout"));
                }
            }
            let _ = bridge.temp_chat(tab_id).await;
            match bridge.wait_temp_chat(tab_id, 20).await {
                Ok(()) => log_llm(format!("phase={} endpoint={} tab={} temp_chat_done", phase, endpoint_id, tab_id)),
                Err(e) => {
                    mark_tab_in_flight(tabs, tab_id, true).await;
                    log_llm(format!("phase={} endpoint={} tab={} temp_chat_timeout={}", phase, endpoint_id, tab_id, e));
                    return Err(anyhow::anyhow!("temp_chat timeout"));
                }
            }
        }
        if tab_cooldown_ms > 0 {
            mark_tab_cooldown(tabs, tab_id, tab_cooldown_ms).await;
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
    tab_cooldown_ms: u64,
    max_retries: u32,
    delay_secs: u64,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        let start = now_ms();
        log_llm(format!("phase={} endpoint={} attempt={} start", phase, endpoint_id, attempt + 1));
        match call_agent_json(bridge, endpoint_id, url, prompt, role_schema, phase, tabs, reuse_tabs, max_tabs, tab_cooldown_ms).await {
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

fn try_parse_loose_json(raw: &str) -> Option<Value> {
    let start = raw.find('{').or_else(|| raw.find('['))?;
    let end = raw.rfind('}').or_else(|| raw.rfind(']'))?;
    if end <= start {
        return None;
    }
    let slice = raw[start..=end].trim();
    serde_json::from_str(slice).ok()
}
