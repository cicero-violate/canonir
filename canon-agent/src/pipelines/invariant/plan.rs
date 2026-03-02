//! Plan phase — LLM interaction, prompt rendering, phase validation, guardrails.

use super::config::{AgentConfig, Phase};
use crate::ir::CodeDelta;
use crate::ws_server::WsBridge;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

// ---------------------------------------------------------------------------
// LLM response shape
// ---------------------------------------------------------------------------

/// Raw deserialized LLM response.
/// `deltas` is optional because `plan` phase emits none.
/// `payload` holds the full JSON for downstream consumers.
#[derive(Debug)]
pub struct AgentResponse {
    pub phase: Phase,
    pub deltas: Vec<CodeDelta>,
    pub rationale: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
struct RawAgentResponse {
    pub phase: Phase,
    #[serde(default)]
    pub deltas: Vec<CodeDelta>,
    #[serde(default)]
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Phase enforcement
// ---------------------------------------------------------------------------

fn allowed_for_phase(phase: &Phase, delta: &CodeDelta) -> bool {
    match phase {
        Phase::Observe => matches!(delta, CodeDelta::BashReadOnly { .. }),
        Phase::Plan    => false, // no deltas permitted
        Phase::Act     => matches!(delta, CodeDelta::Bash { .. } | CodeDelta::ApplyPatch { .. }),
        Phase::Verify  => matches!(delta, CodeDelta::BashReadOnly { .. }),
    }
}

/// Validate phase/delta alignment.
/// Returns (validated_phase, filtered_deltas, Option<warning>).
fn enforce_phase(phase: Phase, deltas: Vec<CodeDelta>) -> (Phase, Vec<CodeDelta>, Option<String>) {
    let violations: Vec<_> = deltas.iter().filter(|d| !allowed_for_phase(&phase, d)).collect();

    if violations.is_empty() {
        return (phase, deltas, None);
    }

    let warn = format!(
        "PHASE_DEMOTION: LLM chose phase={} but emitted {} disallowed delta(s). Demoting to observe, filtering to BashReadOnly only.",
        phase,
        violations.len()
    );
    let filtered: Vec<CodeDelta> = deltas
        .into_iter()
        .filter(|d| matches!(d, CodeDelta::BashReadOnly { .. }))
        .collect();

    (Phase::Observe, filtered, Some(warn))
}

// ---------------------------------------------------------------------------
// Guardrails
// ---------------------------------------------------------------------------

fn check_guardrails(config: &AgentConfig, response: &AgentResponse) -> Result<()> {
    for delta in &response.deltas {
        if let CodeDelta::ApplyPatch { patch } = delta {
            for rule in &config.guardrails {
                let triggered = patch.lines().any(|line| {
                    line.starts_with('+') && line.contains(&rule.forbidden_pattern)
                });
                if triggered {
                    anyhow::bail!("GUARDRAIL_REJECTION: {}", rule.message);
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tab management
// ---------------------------------------------------------------------------

pub async fn get_or_open_tab(
    bridge: &WsBridge,
    url: &str,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
) -> Result<u32> {
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

// ---------------------------------------------------------------------------
// Core plan call
// ---------------------------------------------------------------------------

pub struct PlanRequest<'a> {
    pub tick: u64,
    pub cwd: &'a Path,
    pub bash_output: &'a str,
    pub last_error: &'a str,
    pub rationale_history: &'a str,
    pub exit_check_output: &'a str,
    pub is_bootstrap: bool,
    pub current_phase: Option<&'a Phase>,
}

pub async fn plan_via_llm(
    bridge: &WsBridge,
    config: &AgentConfig,
    req: &PlanRequest<'_>,
    log_dir: &Path,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
) -> Result<AgentResponse> {
    let template = if req.is_bootstrap {
        &config.templates.bootstrap
    } else {
        req.current_phase
            .map(|p| config.template_for_phase(p))
            .unwrap_or(&config.templates.observe)
    };

    let phase_str = req.current_phase.map(|p| p.to_string()).unwrap_or_else(|| "observe".into());

    let prompt = config.render(
        template,
        req.tick,
        &phase_str,
        req.cwd,
        req.bash_output,
        req.last_error,
        req.rationale_history,
        req.exit_check_output,
    );

    std::fs::write(log_dir.join("prompt.txt"), &prompt).ok();

    let tab_id = get_or_open_tab(bridge, &config.chatgpt_url, tab_id_slot).await?;
    let raw = bridge
        .send_turn(tab_id, prompt)
        .await
        .map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;

    let payload = crate::llm_provider::JsonExtractor::extract(&raw)
        .map_err(|e| anyhow::anyhow!("json extract error: {e}"))?;

    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        std::fs::write(log_dir.join("response.json"), pretty).ok();
    }

    parse_and_validate(config, payload)
}

pub async fn plan_via_llm_retry(
    bridge: &WsBridge,
    config: &AgentConfig,
    error: &str,
    log_dir: &Path,
    tab_id_slot: &tokio::sync::Mutex<Option<u32>>,
) -> Result<AgentResponse> {
    let prompt = config.render_retry_addendum(error);
    std::fs::write(log_dir.join("retry_prompt.txt"), &prompt).ok();

    let tab_id = get_or_open_tab(bridge, &config.chatgpt_url, tab_id_slot).await?;
    let raw = bridge
        .send_turn(tab_id, prompt)
        .await
        .map_err(|e| anyhow::anyhow!("llm send_turn error: {e}"))?;

    let payload = crate::llm_provider::JsonExtractor::extract(&raw)
        .map_err(|e| anyhow::anyhow!("json extract error: {e}"))?;

    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        std::fs::write(log_dir.join("retry_response.json"), pretty).ok();
    }

    parse_and_validate(config, payload)
}

// ---------------------------------------------------------------------------
// Parsing + validation
// ---------------------------------------------------------------------------

fn parse_and_validate(config: &AgentConfig, payload: Value) -> Result<AgentResponse> {
    let raw: RawAgentResponse = serde_json::from_value(payload.clone())
        .context("LLM payload did not match AgentResponse schema")?;

    let (phase, deltas, demotion_warn) = enforce_phase(raw.phase, raw.deltas);

    if let Some(warn) = &demotion_warn {
        eprintln!("[agent/plan] {}", warn);
    }

    let response = AgentResponse { phase, deltas, rationale: raw.rationale, payload };
    check_guardrails(config, &response)?;
    Ok(response)
}
