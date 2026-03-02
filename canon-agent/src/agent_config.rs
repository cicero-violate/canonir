//! User-facing agent configuration.
//!
//! Loaded from `agent.json` in the workspace root at startup.
//! `chatgpt_url` is required. Missing or malformed config aborts startup.

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
    /// Load from `<workspace>/agent.json`. Errors if missing or malformed.
    pub fn load(workspace: &Path) -> Result<Self, String> {
        let path = workspace.join("agent.json");
        let raw = std::fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        serde_json::from_slice(&raw).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }
}
