//! Shared LLM call helpers for DAG agents.

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::Mutex;
use std::time::Duration;

use crate::llm_provider::JsonExtractor;
use crate::ws_server::WsBridge;

pub struct DagTabSlots {
    pub decompose: Option<u32>,
    pub planner: Option<u32>,
    pub executor: Option<u32>,
    pub verifier: Option<u32>,
}

impl DagTabSlots {
    pub fn new() -> Self {
        Self { decompose: None, planner: None, executor: None, verifier: None }
    }
}

pub async fn call_agent_json(
    bridge: &WsBridge,
    url: &str,
    role: &str,
    prompt: &str,
    system_prompt: &str,
    tabs: &Mutex<DagTabSlots>,
) -> Result<Value> {
    let tab_id = get_or_open_tab(bridge, url, role, tabs).await?;
    let full_prompt = format!(
        "{}\n\n# Role Output Schema\n{}\n\n# Request\n{}\n\nReturn exactly one fenced ```json block and nothing else.",
        system_prompt.trim_end(),
        role_schema(role),
        prompt.trim_end()
    );
    let raw = bridge.send_turn(tab_id, full_prompt).await.map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;
    let payload = JsonExtractor::extract(&raw).context("json extract error")?;
    Ok(payload)
}

pub async fn call_agent_json_with_retry(
    bridge: &WsBridge,
    url: &str,
    role: &str,
    prompt: &str,
    system_prompt: &str,
    tabs: &Mutex<DagTabSlots>,
    max_retries: u32,
) -> Result<Value> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..max_retries {
        match call_agent_json(bridge, url, role, prompt, system_prompt, tabs).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!("[llm] role={} attempt={} error={}", role, attempt + 1, e);
                last_err = Some(e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("llm retries exhausted")))
}

pub fn schema_mismatch_retry_prompt(role: &str, input: &str, bad_payload: &serde_json::Value) -> String {
    format!(
        "Your previous response did not match the required schema.\n\
Return exactly one fenced ```json block that matches the schema below.\n\
Schema:\n{}\n\n\
Invalid response:\n{}\n\n\
Original input:\n{}",
        role_schema(role),
        serde_json::to_string_pretty(bad_payload).unwrap_or_default(),
        input
    )
}

async fn get_or_open_tab(bridge: &WsBridge, url: &str, role: &str, tabs: &Mutex<DagTabSlots>) -> Result<u32> {
    if let Some(id) = get_tab_id(role, tabs).await {
        return Ok(id);
    }
    bridge.wait_for_connection().await;
    let id = bridge.open_fresh_tab_with_url(url.to_string()).await.map_err(|e| anyhow::anyhow!("failed to open tab: {e}"))?;
    set_tab_id(role, id, tabs).await;
    Ok(id)
}

async fn ensure_system_prompt(_bridge: &WsBridge, _tab_id: u32, _role: &str, _prompt: &str, _tabs: &Mutex<DagTabSlots>) -> Result<()> {
    Ok(())
}

fn role_schema(role: &str) -> &'static str {
    match role {
        "decompose" => r#"{
  "tasks": [
    { "id": "t1", "description": "string", "deps": [] }
  ]
}"#,
        "planner" => r#"{
  "nodes": [
    { "id": "t1", "description": "string", "status": "pending", "deps": [] }
  ]
}"#,
        "executor" => r#"{
  "results": [
    { "id": "t1", "deltas": [ { "type": "write_file", "path": "x", "content": "..." } ], "rationale": "string" }
  ]
}"#,
        "verifier" => r#"{
  "updates": [
    { "id": "t1", "status": "completed", "error": null }
  ]
}"#,
        _ => r#"{}"#,
    }
}

async fn get_tab_id(role: &str, tabs: &Mutex<DagTabSlots>) -> Option<u32> {
    let tabs = tabs.lock().await;
    match role {
        "decompose" => tabs.decompose,
        "planner" => tabs.planner,
        "executor" => tabs.executor,
        "verifier" => tabs.verifier,
        _ => None,
    }
}

async fn set_tab_id(role: &str, id: u32, tabs: &Mutex<DagTabSlots>) {
    let mut tabs = tabs.lock().await;
    match role {
        "decompose" => tabs.decompose = Some(id),
        "planner" => tabs.planner = Some(id),
        "executor" => tabs.executor = Some(id),
        "verifier" => tabs.verifier = Some(id),
        _ => {}
    }
}

pub async fn preflight_tabs(bridge: &WsBridge, roles: &[(&str, &str)], tabs: &Mutex<DagTabSlots>) -> Result<()> {
    let mut ids = Vec::new();
    for (role, url) in roles {
        let tab_id = get_or_open_tab(bridge, url, role, tabs).await?;
        ids.push(tab_id);
    }
    // Send pings sequentially (WS expects one pending response per tab).
    let ping = "Return exactly one fenced ```json block with {\"ok\": true} and nothing else.";
    for tab_id in ids {
        let raw = bridge.send_turn(tab_id, ping.to_string()).await.map_err(|e| anyhow::anyhow!("preflight send_turn error: {e}"))?;
        let _ = JsonExtractor::extract(&raw).context("preflight json extract error")?;
    }
    Ok(())
}
