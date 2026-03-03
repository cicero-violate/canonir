//! Minimal config loader for the invariant pipeline.

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
}

#[derive(Debug, Deserialize, Default)]
struct RawAgents {
    #[serde(default)]
    pub cards: Vec<RawAgentCard>,
}

fn default_max_output_lines() -> usize {
    2000
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
    pub cards: Vec<AgentCard>,
    pub plan_example: String,
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
                let goal_md = std::fs::read_to_string(base_dir.join(&goal_path)).unwrap_or_default();
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
        let plan_example_path = base_dir.join("PLAN_EXAMPLE.json");
        let plan_example = std::fs::read_to_string(plan_example_path).unwrap_or_default();
        Ok(Self {
            exit_check_command: raw.system.exit_check_command,
            max_output_lines: raw.system.max_message_output_lines,
            cards,
            plan_example,
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
}
