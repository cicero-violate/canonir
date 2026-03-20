use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
pub const CAPABILITY_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml";
#[derive(Debug, Deserialize)]
struct CapabilityConfigRawConfig {
    pub system: CapabilityConfigRawSystem,
    #[serde(default)]
    pub llm: CapabilityConfigRawLlm,
}
#[derive(Debug, Deserialize)]
struct CapabilityConfigRawSystem {
    pub exit_check_command: String,
    #[serde(default = "capability_config_default_max_output_lines")]
    pub max_message_output_lines: usize,
    pub goal_file: String,
    #[serde(default = "capability_config_default_max_iterations")]
    pub max_iterations: u64,
    #[serde(default = "capability_config_default_llm_retry_count")]
    pub llm_retry_count: u32,
    #[serde(default = "capability_config_default_llm_retry_delay")]
    pub llm_retry_delay_secs: u64,
    #[serde(default = "capability_config_default_response_timeout_secs")]
    pub response_timeout_secs: u64,
    #[serde(default = "capability_config_default_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "capability_config_default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "capability_config_default_max_expand_iters")]
    pub max_expand_iters: u32,
    #[serde(default = "capability_config_default_context_radius")]
    pub context_radius: usize,
    #[serde(default = "capability_config_default_max_depth")]
    pub max_depth: usize,
    #[serde(default = "capability_config_default_prune_unlinked")]
    pub prune_unlinked: bool,
    #[serde(default = "capability_config_default_planner_max_new_nodes")]
    pub planner_max_new_nodes: usize,
    #[serde(default = "capability_config_default_planner_max_new_edges")]
    pub planner_max_new_edges: usize,
    #[serde(default = "capability_config_default_planner_refine_on_cache")]
    pub planner_refine_on_cache: bool,
    #[serde(default = "capability_config_default_planner_plateau_window")]
    pub planner_plateau_window: usize,
    #[serde(default = "capability_config_default_planner_plateau_threshold")]
    pub planner_plateau_threshold: f64,
    #[serde(default = "capability_config_default_planner_plateau_expand_factor")]
    pub planner_plateau_expand_factor: usize,
    #[serde(default = "capability_config_default_auto_prune")]
    pub auto_prune: bool,
    #[serde(default = "capability_config_default_prune_threshold")]
    pub prune_threshold: f64,
    #[serde(default = "capability_config_default_prune_min_age")]
    pub prune_min_age: u64,
    #[serde(default = "capability_config_default_template_reuse_threshold")]
    pub template_reuse_threshold: f64,
    #[serde(default = "capability_config_default_template_top_k")]
    pub template_top_k: usize,
    #[serde(default = "capability_config_default_recovery_retry_rate_threshold")]
    pub recovery_retry_rate_threshold: f64,
    #[serde(default = "capability_config_default_recovery_failed_fraction_threshold")]
    pub recovery_failed_fraction_threshold: f64,
    #[serde(default = "capability_config_default_max_node_retries")]
    pub max_node_retries: u32,
    #[serde(default = "capability_config_default_repair_radius")]
    pub repair_radius: usize,
    #[serde(default = "capability_config_default_max_repairs_per_node")]
    pub max_repairs_per_node: u32,
    #[serde(default = "capability_config_default_cost_latency_weight")]
    pub cost_latency_weight: f64,
    #[serde(default = "capability_config_default_cost_failure_weight")]
    pub cost_failure_weight: f64,
    #[serde(default = "capability_config_default_cost_decay_rate")]
    pub cost_decay_rate: f64,
    #[serde(default = "capability_config_default_mutation_rate")]
    pub mutation_rate: f64,
    #[serde(default = "capability_config_default_mutation_budget")]
    pub mutation_budget: usize,
    #[serde(default = "capability_config_default_mutation_candidates")]
    pub mutation_candidates: usize,
    #[serde(default = "capability_config_default_template_population_size")]
    pub template_population_size: usize,
    #[serde(default = "capability_config_default_template_failure_hard_ban")]
    pub template_failure_hard_ban: usize,
    #[serde(default = "capability_config_default_failure_constraint_threshold")]
    pub failure_constraint_threshold: usize,
    #[serde(default = "capability_config_default_max_constraints")]
    pub max_constraints: usize,
    #[serde(default = "capability_config_default_enable_resume")]
    pub enable_resume: bool,
    #[serde(default = "capability_config_default_snapshot_interval_iters")]
    pub snapshot_interval_iters: u64,
    #[serde(default = "capability_config_default_snapshot_file")]
    pub snapshot_file: String,
    #[serde(default = "capability_config_default_goal_similarity_weight")]
    pub goal_similarity_weight: f64,
    #[serde(default = "capability_config_default_structural_similarity_weight")]
    pub structural_similarity_weight: f64,
    #[serde(default = "capability_config_default_embedding_model")]
    pub embedding_model: String,
    #[serde(default = "capability_config_default_embedding_dim")]
    pub embedding_dim: usize,
    #[serde(default = "capability_config_default_goal_drift_threshold")]
    pub goal_drift_threshold: f64,
    #[serde(default = "capability_config_default_goal_refocus_strength")]
    pub goal_refocus_strength: f64,
    #[serde(default = "capability_config_default_goal_refocus_rewrite_strength")]
    pub goal_refocus_rewrite_strength: f64,
    #[serde(default = "capability_config_default_reports_disable")]
    pub reports_disable: bool,
    #[serde(default = "capability_config_default_reports_skip_snapshot")]
    pub reports_skip_snapshot: bool,
}
#[derive(Debug, Deserialize, Default)]
struct CapabilityConfigRawLlm {
    #[serde(default)]
    pub endpoints: CapabilityConfigRawEndpoints,
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    #[serde(default = "capability_config_default_tab_cooldown_ms")]
    pub tab_cooldown_ms: u64,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CapabilityConfigRawEndpoints {
    List(Vec<LlmEndpoint>),
    Map(HashMap<String, LlmEndpoint>),
}
impl Default for CapabilityConfigRawEndpoints {
    fn default() -> Self {
        CapabilityConfigRawEndpoints::List(Vec::new())
    }
}
fn capability_config_default_max_output_lines() -> usize {
    2000
}
fn capability_config_default_max_iterations() -> u64 {
    50
}
fn capability_config_default_llm_retry_count() -> u32 {
    3
}
fn capability_config_default_llm_retry_delay() -> u64 {
    5
}
fn capability_config_default_response_timeout_secs() -> u64 {
    60
}
fn capability_config_default_max_concurrency() -> usize {
    4
}
fn capability_config_default_max_nodes() -> usize {
    64
}
fn capability_config_default_max_expand_iters() -> u32 {
    6
}
fn capability_config_default_context_radius() -> usize {
    1
}
fn capability_config_default_max_depth() -> usize {
    6
}
fn capability_config_default_prune_unlinked() -> bool {
    true
}
fn capability_config_default_planner_max_new_nodes() -> usize {
    32
}
fn capability_config_default_planner_max_new_edges() -> usize {
    64
}
fn capability_config_default_planner_refine_on_cache() -> bool {
    true
}
fn capability_config_default_planner_plateau_window() -> usize {
    10
}
fn capability_config_default_planner_plateau_threshold() -> f64 {
    0.01
}
fn capability_config_default_planner_plateau_expand_factor() -> usize {
    2
}
fn capability_config_default_auto_prune() -> bool {
    true
}
fn capability_config_default_prune_threshold() -> f64 {
    0.2
}
fn capability_config_default_prune_min_age() -> u64 {
    5
}
fn capability_config_default_template_reuse_threshold() -> f64 {
    0.15
}
fn capability_config_default_template_top_k() -> usize {
    5
}
fn capability_config_default_recovery_retry_rate_threshold() -> f64 {
    0.3
}
fn capability_config_default_recovery_failed_fraction_threshold() -> f64 {
    0.3
}
fn capability_config_default_max_node_retries() -> u32 {
    3
}
fn capability_config_default_repair_radius() -> usize {
    1
}
fn capability_config_default_max_repairs_per_node() -> u32 {
    3
}
fn capability_config_default_cost_latency_weight() -> f64 {
    0.001
}
fn capability_config_default_cost_failure_weight() -> f64 {
    1.0
}
fn capability_config_default_cost_decay_rate() -> f64 {
    0.2
}
fn capability_config_default_mutation_rate() -> f64 {
    0.25
}
fn capability_config_default_mutation_budget() -> usize {
    2
}
fn capability_config_default_mutation_candidates() -> usize {
    2
}
fn capability_config_default_template_population_size() -> usize {
    5
}
fn capability_config_default_template_failure_hard_ban() -> usize {
    6
}
fn capability_config_default_failure_constraint_threshold() -> usize {
    2
}
fn capability_config_default_max_constraints() -> usize {
    16
}
fn capability_config_default_enable_resume() -> bool {
    true
}
fn capability_config_default_snapshot_interval_iters() -> u64 {
    10
}
fn capability_config_default_snapshot_file() -> String {
    "/workspace/ai_sandbox/canon/agent_logs/state_snapshot.json".to_string()
}
fn capability_config_default_goal_similarity_weight() -> f64 {
    0.6
}
fn capability_config_default_structural_similarity_weight() -> f64 {
    0.4
}
fn capability_config_default_embedding_model() -> String {
    "hash".to_string()
}
fn capability_config_default_embedding_dim() -> usize {
    64
}
fn capability_config_default_goal_drift_threshold() -> f64 {
    0.6
}
fn capability_config_default_goal_refocus_strength() -> f64 {
    0.5
}
fn capability_config_default_goal_refocus_rewrite_strength() -> f64 {
    0.3
}
fn capability_config_default_reports_disable() -> bool {
    false
}
fn capability_config_default_reports_skip_snapshot() -> bool {
    false
}
fn capability_config_default_max_tabs() -> usize {
    1
}
fn capability_config_default_tab_cooldown_ms() -> u64 {
    0
}
#[derive(Debug, Deserialize, Clone, Default)]
pub struct RoleConfig {
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
    #[serde(default = "capability_config_default_max_tabs")]
    pub max_tabs: usize,
}
#[derive(Clone)]
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
    pub template_reuse_threshold: f64,
    pub template_top_k: usize,
    pub recovery_retry_rate_threshold: f64,
    pub recovery_failed_fraction_threshold: f64,
    pub max_node_retries: u32,
    pub repair_radius: usize,
    pub max_repairs_per_node: u32,
    pub cost_latency_weight: f64,
    pub cost_failure_weight: f64,
    pub cost_decay_rate: f64,
    pub mutation_rate: f64,
    pub mutation_budget: usize,
    pub mutation_candidates: usize,
    pub template_population_size: usize,
    pub template_failure_hard_ban: usize,
    pub failure_constraint_threshold: usize,
    pub max_constraints: usize,
    pub enable_resume: bool,
    pub snapshot_interval_iters: u64,
    pub snapshot_file: String,
    pub goal_similarity_weight: f64,
    pub structural_similarity_weight: f64,
    pub embedding_model: String,
    pub embedding_dim: usize,
    pub goal_drift_threshold: f64,
    pub goal_refocus_strength: f64,
    pub goal_refocus_rewrite_strength: f64,
    pub reports_disable: bool,
    pub reports_skip_snapshot: bool,
    pub llm_endpoints: Vec<LlmEndpoint>,
    pub planner_endpoint: Option<LlmEndpoint>,
    pub llm_roles: HashMap<String, RoleConfig>,
    pub tab_cooldown_ms: u64,
}
impl CapabilityConfig {
    pub fn snapshot_store_load() -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_CONFIG_TOML).with_context(|| format!("cannot read {}", CAPABILITY_CONFIG_TOML))?;
        let raw: CapabilityConfigRawConfig = toml::from_str(&raw_toml).context("cannot parse capability_config.toml")?;
        let (llm_endpoints, planner_endpoint) = match raw.llm.endpoints {
            CapabilityConfigRawEndpoints::List(list) => {
                let planner = list.iter().find(|e| e.role.as_deref() == Some("planner")).cloned();
                (list, planner)
            }
            CapabilityConfigRawEndpoints::Map(map) => {
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
            template_reuse_threshold: raw.system.template_reuse_threshold,
            template_top_k: raw.system.template_top_k,
            recovery_retry_rate_threshold: raw.system.recovery_retry_rate_threshold,
            recovery_failed_fraction_threshold: raw.system.recovery_failed_fraction_threshold,
            max_node_retries: raw.system.max_node_retries,
            repair_radius: raw.system.repair_radius,
            max_repairs_per_node: raw.system.max_repairs_per_node,
            cost_latency_weight: raw.system.cost_latency_weight,
            cost_failure_weight: raw.system.cost_failure_weight,
            cost_decay_rate: raw.system.cost_decay_rate,
            mutation_rate: raw.system.mutation_rate,
            mutation_budget: raw.system.mutation_budget,
            mutation_candidates: raw.system.mutation_candidates,
            template_population_size: raw.system.template_population_size,
            template_failure_hard_ban: raw.system.template_failure_hard_ban,
            failure_constraint_threshold: raw.system.failure_constraint_threshold,
            max_constraints: raw.system.max_constraints,
            enable_resume: raw.system.enable_resume,
            snapshot_interval_iters: raw.system.snapshot_interval_iters,
            snapshot_file: raw.system.snapshot_file,
            goal_similarity_weight: raw.system.goal_similarity_weight,
            structural_similarity_weight: raw.system.structural_similarity_weight,
            embedding_model: raw.system.embedding_model,
            embedding_dim: raw.system.embedding_dim,
            goal_drift_threshold: raw.system.goal_drift_threshold,
            goal_refocus_strength: raw.system.goal_refocus_strength,
            goal_refocus_rewrite_strength: raw.system.goal_refocus_rewrite_strength,
            reports_disable: raw.system.reports_disable,
            reports_skip_snapshot: raw.system.reports_skip_snapshot,
            llm_endpoints,
            planner_endpoint,
            llm_roles: raw.llm.roles,
            tab_cooldown_ms: raw.llm.tab_cooldown_ms,
        })
    }
    pub fn apply_env_flags(&self) {
        unsafe {
            std::env::set_var("CANON_REPORTS_DISABLE", if self.reports_disable { "1" } else { "0" });
            std::env::set_var("CANON_REPORTS_SKIP_SNAPSHOT", if self.reports_skip_snapshot { "1" } else { "0" });
        }
    }
    pub fn endpoint_by_id(&self, id: &str) -> Result<&LlmEndpoint> {
        self.llm_endpoints.iter().find(|e| e.id == id).ok_or_else(|| anyhow::anyhow!("no llm endpoint for id={}", id))
    }
    pub fn role_config(&self, role: &str) -> RoleConfig {
        self.llm_roles.get(role).cloned().unwrap_or_default()
    }
    pub fn planner_endpoint(&self) -> Result<&LlmEndpoint> {
        self.planner_endpoint.as_ref().ok_or_else(|| anyhow::anyhow!("no planner endpoint configured"))
    }
}
const CAPABILITY_POLICY_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_policy.toml";
#[derive(Debug, Deserialize)]
struct CapabilityConfigRawPolicy {
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
        Self { write_allowed_roots: Vec::new(), require_final_render: false, max_node_retries: capability_config_default_max_node_retries() }
    }
}
impl CapabilityPolicy {
    pub fn snapshot_store_load(workspace_root: &Path) -> Result<Self> {
        let raw_toml = std::fs::read_to_string(CAPABILITY_POLICY_TOML).with_context(|| format!("cannot read {}", CAPABILITY_POLICY_TOML))?;
        let raw: CapabilityConfigRawPolicy = toml::from_str(&raw_toml).context("cannot parse capability_policy.toml")?;
        let roots = raw
            .write_allowed_roots
            .into_iter()
            .map(|p| {
                let path = Path::new(&p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    workspace_root.join(path)
                }
            })
            .collect::<Vec<_>>();
        Ok(Self { write_allowed_roots: roots, require_final_render: raw.require_final_render, max_node_retries: capability_config_default_max_node_retries() })
    }
}
