//! Minimal config loader for the multi-dag pipeline.

use anyhow::{Context, Result};
use serde::Deserialize;

pub const AGENT_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/agent_config.toml";

#[derive(Debug, Deserialize)]
struct RawAgentConfig {
    pub system: RawSystem,
    #[serde(default)]
    pub agents: RawAgents,
}

#[derive(Debug, Deserialize)]
struct RawSystem {
    pub exit_check_command: String,
    #[serde(default = "default_max_output_lines")]
    pub max_message_output_lines: usize,
    #[serde(default = "default_response_timeout_secs")]
    pub response_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Default)]
struct RawAgents {
    #[serde(default)]
    pub cards: Vec<RawAgentCard>,
}

fn default_max_output_lines() -> usize {
    2000
}
fn default_response_timeout_secs() -> u64 {
    60
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawAgentCard {
    pub agent_url: String,
    pub agent_id: String,
    pub role: String,
    pub goal: String,
    pub tool_capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AgentCard {
    pub agent_url: String,
    pub agent_id: String,
    pub role_path: String,
    pub goal_path: String,
    pub role_markdown: String,
    pub goal_markdown: String,
    pub tool_capabilities: Vec<String>,
}

pub struct AgentConfig {
    pub exit_check_command: String,
    pub max_output_lines: usize,
    pub response_timeout_secs: u64,
    pub cards: Vec<AgentCard>,
}

impl AgentConfig {
    pub fn load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(AGENT_CONFIG_TOML).with_context(|| format!("cannot read {}", AGENT_CONFIG_TOML))?;
        let raw: RawAgentConfig = toml::from_str(&raw_toml).context("cannot parse agent_config.toml")?;
        let base_dir = std::path::Path::new(AGENT_CONFIG_TOML)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("/workspace/ai_sandbox/canon/canon-agent-prompts"));
        let mut cards: Vec<AgentCard> = raw
            .agents
            .cards
            .into_iter()
            .map(|c| {
                let role_path = c.role.clone();
                let goal_path = c.goal.clone();
                let role_md = std::fs::read_to_string(base_dir.join(&role_path)).unwrap_or_default();
                let mut goal_md = std::fs::read_to_string(base_dir.join(&goal_path)).unwrap_or_default();
                if goal_md.trim().is_empty() {
                    let fallback_goal = base_dir.join("GOAL.md");
                    goal_md = std::fs::read_to_string(fallback_goal).unwrap_or_default();
                }
                if goal_md.trim().is_empty() {
                    let fallback_goal = base_dir.join("AGENT_GOAL.md");
                    goal_md = std::fs::read_to_string(fallback_goal).unwrap_or_default();
                }
                AgentCard {
                    agent_url: c.agent_url,
                    agent_id: c.agent_id,
                    role_path,
                    goal_path,
                    role_markdown: role_md,
                    goal_markdown: goal_md,
                    tool_capabilities: c.tool_capabilities,
                }
            })
            .collect();
        Ok(Self {
            exit_check_command: raw.system.exit_check_command,
            max_output_lines: raw.system.max_message_output_lines,
            response_timeout_secs: raw.system.response_timeout_secs,
            cards,
        })
    }

    pub fn primary_agent_url(&self) -> Result<&str> {
        self.cards
            .first()
            .map(|c| c.agent_url.as_str())
            .ok_or_else(|| anyhow::anyhow!("no agent cards configured in agent_config.toml"))
    }

    pub fn primary_card(&self) -> Result<&AgentCard> {
        self.cards
            .first()
            .ok_or_else(|| anyhow::anyhow!("no agent cards configured in agent_config.toml"))
    }

    pub fn card_by_role(&self, role: &str) -> Result<&AgentCard> {
        self.cards
            .iter()
            .find(|c| c.agent_id == role)
            .ok_or_else(|| anyhow::anyhow!("no agent card for role={}", role))
    }
}
