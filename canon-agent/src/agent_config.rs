//! User-facing agent configuration.
//!
//! Loaded from `canon-agent-prompts/agent_config.toml`.
//! `agents.cards[0].agent_url` is required. Missing or malformed config aborts startup.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    /// ChatGPT tab URL opened for every LLM call. Required.
    pub chatgpt_url: String,

    /// Override max ticks (0 = run forever).
    pub max_ticks: Option<u64>,

    /// Meta-tick interval.
    pub meta_tick_interval: Option<u64>,

    /// Policy update interval.
    pub policy_update_interval: Option<u64>,
}

impl AgentConfig {
    /// Load from `<workspace>/canon-agent-prompts/agent_config.toml`. Errors if missing or malformed.
    pub fn load(workspace: &Path) -> Result<Self, String> {
        let primary = workspace.join("canon-agent-prompts").join("agent_config.toml");
        let fallback = workspace.join("..").join("canon-agent-prompts").join("agent_config.toml");
        let (path, raw) = match std::fs::read_to_string(&primary) {
            Ok(raw) => (primary, raw),
            Err(_) => {
                let raw = std::fs::read_to_string(&fallback)
                    .map_err(|e| format!("failed to read {}: {e}", fallback.display()))?;
                (fallback, raw)
            }
        };
        let cfg: RawAgentConfig = toml::from_str(&raw).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
        let chatgpt_url = cfg
            .agents
            .cards
            .first()
            .map(|c| c.agent_url.clone())
            .ok_or_else(|| format!("agent_config.toml missing agents.cards[0].agent_url"))?;
        Ok(AgentConfig {
            chatgpt_url,
            max_ticks: cfg.runner.as_ref().and_then(|r| r.max_ticks),
            meta_tick_interval: cfg.runner.as_ref().and_then(|r| r.meta_tick_interval),
            policy_update_interval: cfg.runner.as_ref().and_then(|r| r.policy_update_interval),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawAgentConfig {
    #[serde(default)]
    pub runner: Option<RawRunner>,
    pub agents: RawAgents,
}

#[derive(Debug, Deserialize)]
struct RawRunner {
    pub max_ticks: Option<u64>,
    pub meta_tick_interval: Option<u64>,
    pub policy_update_interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawAgents {
    pub cards: Vec<RawAgentCard>,
}

#[derive(Debug, Deserialize)]
struct RawAgentCard {
    pub agent_url: String,
}
