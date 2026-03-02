//! Agent pipeline configuration — loads agent_config.toml.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const AGENT_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/agent_config.toml";
pub const AGENT_PROMPTS_DIR: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts";

// ---------------------------------------------------------------------------
// Raw TOML shape
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RawAgentConfig {
    pub agent: RawAgent,
}

#[derive(Debug, Deserialize)]
pub struct RawAgent {
    pub chatgpt_url: String,
    pub max_ticks: usize,
    pub exit_check_command: String,
    pub rationale_history_len: usize,
    pub templates: RawTemplates,
    pub retry_addendum: String,
    #[serde(default)]
    pub guardrails: Vec<RawGuardrail>,
}

#[derive(Debug, Deserialize)]
pub struct RawTemplates {
    pub bootstrap: String,
    pub observe: String,
    pub plan: String,
    pub act: String,
    pub verify: String,
}

#[derive(Debug, Deserialize)]
pub struct RawGuardrail {
    pub forbidden_pattern: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Resolved config
// ---------------------------------------------------------------------------

pub struct AgentConfig {
    pub chatgpt_url: String,
    pub max_ticks: usize,
    pub exit_check_command: String,
    pub rationale_history_len: usize,
    pub templates: Templates,
    pub retry_addendum: String,
    pub guardrails: Vec<RawGuardrail>,
}

pub struct Templates {
    pub bootstrap: String,
    pub observe: String,
    pub plan: String,
    pub act: String,
    pub verify: String,
}

impl AgentConfig {
    pub fn load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(AGENT_CONFIG_TOML)
            .with_context(|| format!("cannot read {}", AGENT_CONFIG_TOML))?;
        let raw: RawAgentConfig = toml::from_str(&raw_toml)
            .context("cannot parse agent_config.toml")?;
        let a = raw.agent;
        let dir = Path::new(AGENT_PROMPTS_DIR);

        let load = |name: &str| -> Result<String> {
            std::fs::read_to_string(dir.join(name))
                .with_context(|| format!("cannot read template: {}", name))
        };

        Ok(Self {
            chatgpt_url: a.chatgpt_url,
            max_ticks: a.max_ticks,
            exit_check_command: a.exit_check_command,
            rationale_history_len: a.rationale_history_len,
            retry_addendum: a.retry_addendum,
            guardrails: a.guardrails,
            templates: Templates {
                bootstrap: load(&a.templates.bootstrap)?,
                observe:   load(&a.templates.observe)?,
                plan:      load(&a.templates.plan)?,
                act:       load(&a.templates.act)?,
                verify:    load(&a.templates.verify)?,
            },
        })
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    pub fn render(
        &self,
        template: &str,
        tick: u64,
        phase: &str,
        cwd: &Path,
        bash_output: &str,
        last_error: &str,
        rationale_history: &str,
        exit_check_output: &str,
    ) -> String {
        template
            .replace("{{TICK}}", &tick.to_string())
            .replace("{{PHASE}}", phase)
            .replace("{{CWD}}", &cwd.display().to_string())
            .replace("{{BASH_OUTPUT}}", bash_output)
            .replace("{{LAST_ERROR}}", last_error)
            .replace("{{RATIONALE_HISTORY}}", rationale_history)
            .replace("{{EXIT_CHECK_OUTPUT}}", exit_check_output)
    }

    pub fn render_retry_addendum(&self, error: &str) -> String {
        self.retry_addendum.replace("{{RETRY_ERROR}}", error)
    }

    pub fn template_for_phase(&self, phase: &Phase) -> &str {
        match phase {
            Phase::Observe => &self.templates.observe,
            Phase::Plan    => &self.templates.plan,
            Phase::Act     => &self.templates.act,
            Phase::Verify  => &self.templates.verify,
        }
    }
}

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Observe,
    Plan,
    Act,
    Verify,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Observe => write!(f, "observe"),
            Phase::Plan    => write!(f, "plan"),
            Phase::Act     => write!(f, "act"),
            Phase::Verify  => write!(f, "verify"),
        }
    }
}
