use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    #[serde(default = "default_planner_max_new_nodes")]
    pub planner_max_new_nodes: usize,
    #[serde(default = "default_planner_max_new_edges")]
    pub planner_max_new_edges: usize,
    #[serde(default = "default_planner_refine_on_cache")]
    pub planner_refine_on_cache: bool,
    #[serde(default = "default_planner_plateau_window")]
    pub planner_plateau_window: usize,
    #[serde(default = "default_planner_plateau_threshold")]
    pub planner_plateau_threshold: f64,
    #[serde(default = "default_planner_plateau_expand_factor")]
    pub planner_plateau_expand_factor: usize,
    #[serde(default = "default_auto_prune")]
    pub auto_prune: bool,
    #[serde(default = "default_prune_threshold")]
    pub prune_threshold: f64,
    #[serde(default = "default_prune_min_age")]
    pub prune_min_age: u64,
    #[serde(default = "default_max_node_retries")]
    pub max_node_retries: u32,
}

#[derive(Debug, Deserialize, Default)]
struct RawLlm {
    #[serde(default)]
    pub endpoints: RawEndpoints,
    #[serde(default)]
    pub roles: HashMap<String, RawRoleConfig>,
    #[serde(default = "default_tab_cooldown_ms")]
    pub tab_cooldown_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawEndpoints {
    List(Vec<LlmEndpoint>),
    Map(HashMap<String, LlmEndpoint>),
}

impl Default for RawEndpoints {
    fn default() -> Self {
        RawEndpoints::List(Vec::new())
    }
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
fn default_planner_max_new_nodes() -> usize { 32 }
fn default_planner_max_new_edges() -> usize { 64 }
fn default_planner_refine_on_cache() -> bool { true }
fn default_planner_plateau_window() -> usize { 10 }
fn default_planner_plateau_threshold() -> f64 { 0.01 }
fn default_planner_plateau_expand_factor() -> usize { 2 }
fn default_auto_prune() -> bool { true }
fn default_prune_threshold() -> f64 { 0.2 }
fn default_prune_min_age() -> u64 { 5 }
fn default_max_node_retries() -> u32 { 3 }
fn default_max_tabs() -> usize { 1 }
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
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub stateful: bool,
    #[serde(default = "default_max_tabs")]
    pub max_tabs: usize,
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
    pub planner_max_new_nodes: usize,
    pub planner_max_new_edges: usize,
    pub planner_refine_on_cache: bool,
    pub planner_plateau_window: usize,
    pub planner_plateau_threshold: f64,
    pub planner_plateau_expand_factor: usize,
    pub auto_prune: bool,
    pub prune_threshold: f64,
    pub prune_min_age: u64,
    pub max_node_retries: u32,
    pub llm_endpoints: Vec<LlmEndpoint>,
    pub planner_endpoint: Option<LlmEndpoint>,
    pub llm_roles: HashMap<String, RawRoleConfig>,
    pub tab_cooldown_ms: u64,
}

impl CapabilityConfig {
    pub fn load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_CONFIG_TOML).with_context(|| format!("cannot read {}", CAPABILITY_CONFIG_TOML))?;
        let raw: RawConfig = toml::from_str(&raw_toml).context("cannot parse capability_config.toml")?;
        let (llm_endpoints, planner_endpoint) = match raw.llm.endpoints {
            RawEndpoints::List(list) => {
                let planner = list.iter().find(|e| e.role.as_deref() == Some("planner")).cloned();
                (list, planner)
            }
            RawEndpoints::Map(map) => {
                let mut list = Vec::new();
                let mut planner = None;
                for (key, mut ep) in map {
                    if ep.id.is_empty() {
                        ep.id = key.clone();
                    }
                    if ep.role.is_none() && key == "planner" {
                        ep.role = Some("planner".to_string());
                    }
                    if ep.role.as_deref() == Some("planner") {
                        planner = Some(ep.clone());
                    }
                    list.push(ep);
                }
                (list, planner)
            }
        };
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
            planner_max_new_nodes: raw.system.planner_max_new_nodes,
            planner_max_new_edges: raw.system.planner_max_new_edges,
            planner_refine_on_cache: raw.system.planner_refine_on_cache,
            planner_plateau_window: raw.system.planner_plateau_window,
            planner_plateau_threshold: raw.system.planner_plateau_threshold,
            planner_plateau_expand_factor: raw.system.planner_plateau_expand_factor,
            auto_prune: raw.system.auto_prune,
            prune_threshold: raw.system.prune_threshold,
            prune_min_age: raw.system.prune_min_age,
            max_node_retries: raw.system.max_node_retries,
            llm_endpoints,
            planner_endpoint,
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

    pub fn planner_endpoint(&self) -> Result<&LlmEndpoint> {
        self.planner_endpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no planner endpoint configured"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    pub raw: String,
}

impl GoalSpec {
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    pub fn from_file(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(path).with_context(|| format!("failed to read goal file: {}", path))?;
        Ok(Self::new(raw))
    }
}

const CAPABILITY_POLICY_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_policy.toml";

#[derive(Debug, Deserialize)]
struct RawPolicy {
    #[serde(default)]
    pub write_allowed_roots: Vec<String>,
    #[serde(default)]
    pub require_final_render: bool,
}

#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    pub write_allowed_roots: Vec<PathBuf>,
    pub require_final_render: bool,
    pub max_node_retries: u32,
}
impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            write_allowed_roots: Vec::new(),
            require_final_render: false,
            max_node_retries: default_max_node_retries(),
        }
    }
}

impl CapabilityPolicy {
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_POLICY_TOML)
            .with_context(|| format!("cannot read {}", CAPABILITY_POLICY_TOML))?;
        let raw: RawPolicy = toml::from_str(&raw_toml).context("cannot parse capability_policy.toml")?;
        let roots = raw
            .write_allowed_roots
            .into_iter()
            .map(|p| {
                let path = Path::new(&p);
                if path.is_absolute() { path.to_path_buf() } else { workspace_root.join(path) }
            })
            .collect::<Vec<_>>();
        Ok(Self { write_allowed_roots: roots, require_final_render: raw.require_final_render, max_node_retries: default_max_node_retries() })
        // max_node_retries is patched in by mod.rs after loading CapabilityConfig
    }
}
