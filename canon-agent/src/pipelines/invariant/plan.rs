//! LLM interaction for the invariant pipeline.

use super::{Delta, Phase};
use crate::llm_provider::JsonExtractor;
use crate::ws_server::WsBridge;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentResponse {
    pub phase: Phase,
    #[serde(default)]
    pub deltas: Vec<Delta>,
    #[serde(default)]
    pub rationale: String,
}

pub async fn request_plan(
    bridge: &WsBridge,
    config: &crate::pipelines::invariant::config::AgentConfig,
    prompt: &str,
    tab_id_slot: &Mutex<Option<u32>>,
) -> Result<(Value, AgentResponse)> {
    let url = config.primary_agent_url()?;
    let tab_id = get_or_open_tab(bridge, url, tab_id_slot).await?;
    let raw = bridge.send_turn(tab_id, prompt.to_string()).await.map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;
    let payload = JsonExtractor::extract(&raw).context("json extract error")?;
    let parsed: AgentResponse = serde_json::from_value(payload.clone()).context("LLM payload did not match AgentResponse schema")?;
    Ok((payload, parsed))
}

pub async fn send_system_prompt(
    bridge: &WsBridge,
    config: &crate::pipelines::invariant::config::AgentConfig,
    tab_id_slot: &Mutex<Option<u32>>,
    prompt: &str,
) -> Result<()> {
    let url = config.primary_agent_url()?;
    let tab_id = get_or_open_tab(bridge, url, tab_id_slot).await?;
    let _ = bridge.send_turn(tab_id, prompt.to_string()).await.map_err(|e| anyhow::anyhow!("system prompt send_turn error: {e}"))?;
    Ok(())
}

async fn get_or_open_tab(bridge: &WsBridge, url: &str, tab_id_slot: &Mutex<Option<u32>>) -> Result<u32> {
    let mut slot = tab_id_slot.lock().await;
    if let Some(id) = *slot {
        return Ok(id);
    }
    bridge.wait_for_connection().await;
    let id = bridge
        .open_fresh_tab_with_url(url.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("failed to open tab: {e}"))?;
    *slot = Some(id);
    Ok(id)
}
