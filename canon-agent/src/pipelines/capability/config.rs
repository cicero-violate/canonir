use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

pub const CAPABILITY_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml";

#[derive(Debug, Deserialize)]
struct RawConfig {
    pub system: RawSystem,
    #[serde(default)]
    pub llm: RawLlm,
}

#[derive(Debug, Deserialize)]
struct RawSystem {
    pub exit_check_command: String,
    #[serde(default = "default_max_output_lines")]
    pub max_message_output_lines: usize,
    pub goal_file: String,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u64,
    #[serde(default = "default_llm_retry_count")]
    pub llm_retry_count: u32,
    #[serde(default = "default_llm_retry_delay")]
    pub llm_retry_delay_secs: u64,
    #[serde(default = "default_response_timeout_secs")]
    pub response_timeout_secs: u64,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_max_expand_iters")]
    pub max_expand_iters: u32,
    #[serde(default = "default_context_radius")]
    pub context_radius: usize,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_prune_unlinked")]
    pub prune_unlinked: bool,
}

#[derive(Debug, Deserialize, Default)]
struct RawLlm {
    #[serde(default)]
    pub endpoints: Vec<LlmEndpoint>,
    #[serde(default)]
    pub roles: HashMap<String, RawRoleConfig>,
    #[serde(default = "default_tab_cooldown_ms")]
    pub tab_cooldown_ms: u64,
}

fn default_max_output_lines() -> usize { 2000 }
fn default_max_iterations() -> u64 { 50 }
fn default_llm_retry_count() -> u32 { 3 }
fn default_llm_retry_delay() -> u64 { 5 }
fn default_response_timeout_secs() -> u64 { 60 }
fn default_max_concurrency() -> usize { 4 }
fn default_max_nodes() -> usize { 64 }
fn default_max_expand_iters() -> u32 { 6 }
fn default_context_radius() -> usize { 1 }
fn default_max_depth() -> usize { 6 }
fn default_prune_unlinked() -> bool { true }
fn default_max_tabs() -> usize { 1 }
fn default_reuse_tabs() -> bool { true }
fn default_tab_cooldown_ms() -> u64 { 0 }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RawRoleConfig {
    #[serde(default)]
    pub weights: HashMap<String, u32>,
    pub burst: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmEndpoint {
    pub id: String,
    pub url: String,
    pub role_markdown: String,
    #[serde(default = "default_max_tabs")]
    pub max_tabs: usize,
    #[serde(default = "default_reuse_tabs")]
    pub reuse_tabs: bool,
}

pub struct CapabilityConfig {
    pub exit_check_command: String,
    pub max_output_lines: usize,
    pub goal_file: String,
    pub max_iterations: u64,
    pub llm_retry_count: u32,
    pub llm_retry_delay_secs: u64,
    pub response_timeout_secs: u64,
    pub max_concurrency: usize,
    pub max_nodes: usize,
    pub max_expand_iters: u32,
    pub context_radius: usize,
    pub max_depth: usize,
    pub prune_unlinked: bool,
    pub llm_endpoints: Vec<LlmEndpoint>,
    pub llm_roles: HashMap<String, RawRoleConfig>,
    pub tab_cooldown_ms: u64,
}

impl CapabilityConfig {
    pub fn load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_CONFIG_TOML).with_context(|| format!("cannot read {}", CAPABILITY_CONFIG_TOML))?;
        let raw: RawConfig = toml::from_str(&raw_toml).context("cannot parse capability_config.toml")?;
        Ok(Self {
            exit_check_command: raw.system.exit_check_command,
            max_output_lines: raw.system.max_message_output_lines,
            goal_file: raw.system.goal_file,
            max_iterations: raw.system.max_iterations,
            llm_retry_count: raw.system.llm_retry_count,
            llm_retry_delay_secs: raw.system.llm_retry_delay_secs,
            response_timeout_secs: raw.system.response_timeout_secs,
            max_concurrency: raw.system.max_concurrency,
            max_nodes: raw.system.max_nodes,
            max_expand_iters: raw.system.max_expand_iters,
            context_radius: raw.system.context_radius,
            max_depth: raw.system.max_depth,
            prune_unlinked: raw.system.prune_unlinked,
            llm_endpoints: raw.llm.endpoints,
            llm_roles: raw.llm.roles,
            tab_cooldown_ms: raw.llm.tab_cooldown_ms,
        })
    }

    pub fn endpoint_by_id(&self, id: &str) -> Result<&LlmEndpoint> {
        self.llm_endpoints
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("no llm endpoint for id={}", id))
    }

    pub fn role_config(&self, role: &str) -> RawRoleConfig {
        self.llm_roles.get(role).cloned().unwrap_or_default()
    }
}
