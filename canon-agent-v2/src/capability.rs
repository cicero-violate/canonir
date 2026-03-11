//! Capability-driven DAG pipeline.
pub use crate::act;
pub use crate::capability_cost;
pub use crate::capability_types;
pub use crate::capability_types as capability;
pub use crate::config;
pub use crate::console;
pub use crate::dag;
pub use crate::decompose;
pub use crate::dispatch;
pub use crate::endpoint_scheduler;
pub use crate::endpoint_worker;
pub use crate::engine;
pub use crate::execution_result;
pub use crate::executor_dispatch;
pub use crate::failure_store;
pub use crate::goal;
pub use crate::goal_embedding;
pub use crate::gpu_scheduler;
pub use crate::graph_algo;
pub use crate::graph_maintenance;
pub use crate::graph_runtime;
pub use crate::llm;
pub use crate::planner_session;
pub use crate::planner_state;
pub use crate::planner_update;
pub use crate::policy;
pub use crate::policy_engine;
pub use crate::policy_train;
pub use crate::response_router;
pub use crate::scheduler;
pub use crate::scheduler_scoring;
pub use crate::scheduler_state;
pub use crate::state_snapshot;
pub use crate::telemetry;
pub use crate::objectives;
pub use crate::template_index;
pub use crate::template_mutation;
pub use crate::templates;
pub use crate::tab_management;
pub use crate::capability_types::{
    capability_model_assert_class_disjoint, capability_model_dominant_class,
    CapabilityMode, PipelineCapability,
};
use crate::ir::{IntentStatePersist, SystemState};
use crate::layout::FileTopology;
use crate::planner_state::{PlannerStage, PlannerStagePersist};
use crate::pipelines_core_4::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::{CapabilityConfig, CapabilityConfigGoalSpec};
use crate::goal::GoalSpec;
use graph_algo::{graph_analysis_emit_planned_graph, graph_analysis_run_graph_algorithms};
use policy::ExecutionPolicyModel;
use policy_train::PolicyTrainingPolicyDatasetEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use templates::GraphTemplateStore;
pub const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";
pub const TEMPLATE_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/templates";
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionDelta {
    ReadFile { path: String },
    ListDir { path: String },
    ReadCommand { command: String, args: Vec<String>, #[serde(default)] path: Option<String> },
    WriteFile { path: String, content: String },
    ReplaceText { path: String, find: String, replace: String },
    DeleteFile { path: String },
}
pub struct CapabilityPipeline {
    bridge: WsBridge,
    config: CapabilityConfig,
    tabs: engine::TabManagerHandle,
    role_rr: tokio::sync::Mutex<HashMap<String, usize>>,
}
impl CapabilityPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = CapabilityConfig::snapshot_store_load()
            .expect("failed to load capability config");
        Self {
            bridge,
            config,
            tabs: engine::llm_worker_new_tabs(),
            role_rr: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
    fn ensure_log_dir() {
        let _ = std::fs::create_dir_all(LOG_ROOT);
        let _ = std::fs::create_dir_all("/workspace/ai_sandbox/canon/agent_logs");
        let _ = std::fs::create_dir_all(
            "/workspace/ai_sandbox/canon/agent_logs/templates",
        );
        Self::ensure_agent_log_files();
    }
    fn ensure_agent_log_files() {
        Self::ensure_file(
            "/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl",
            "",
        );
        Self::ensure_file(
            "/workspace/ai_sandbox/canon/agent_logs/goal_embeddings.json",
            "{}",
        );
        Self::ensure_file("/workspace/ai_sandbox/canon/agent_logs/metrics.json", "{}");
        Self::ensure_file(
            "/workspace/ai_sandbox/canon/agent_logs/capability_costs.json",
            "{}",
        );
        let weights_path = Path::new(
            "/workspace/ai_sandbox/canon/agent_logs/policy_weights.json",
        );
        if !weights_path.exists() {
            let model = ExecutionPolicyModel::load_default();
            let _ = model.snapshot_store_save(weights_path);
        }
    }
    fn ensure_file(path: &str, contents: &str) {
        let p = Path::new(path);
        if !p.exists() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(p, contents);
        }
    }
    fn log_path(name: &str) -> PathBuf {
        Path::new(LOG_ROOT).join(name)
    }
    pub async fn run_capability_loop(&self, ctx: &PipelineContext) -> Result<f64> {
        Self::ensure_log_dir();
        if self.config.llm_endpoints.is_empty() {
            anyhow::bail!("capability config has no llm endpoints");
        }
        engine::module_init_io_workers(&self.bridge, &self.config, &self.tabs).await;
        let mut goal = CapabilityConfigGoalSpec::from_file(&self.config.goal_file)?;
        let intent_path = Path::new("/workspace/ai_sandbox/canon/kernel/state/intent_state.json");
        if let Some(intent) = IntentStatePersist::load(intent_path) {
            if !intent.goal.trim().is_empty() {
                goal.raw = intent.goal;
            } else if !goal.raw.trim().is_empty() {
                let updated = IntentStatePersist {
                    goal: goal.raw.clone(),
                    intent_radius: intent.intent_radius,
                    execution_budget: intent.execution_budget,
                };
                updated.save(intent_path);
            }
        } else if !goal.raw.trim().is_empty() {
            let updated = IntentStatePersist {
                goal: goal.raw.clone(),
                intent_radius: 0,
                execution_budget: 0,
            };
            updated.save(intent_path);
        }
        let selection = objectives::load_goal_from_reports(objectives::ObjectiveWeights::default());
        if let Some(selection) = selection.as_ref() {
            goal.raw = objectives::goal_raw_with_artifact(&goal.raw, &selection.artifact);
        }
        let goal_spec = GoalSpec::new_with_artifact(
            goal.raw.clone(),
            graph_algo::graph_embedding_dim(),
            selection.map(|s| s.artifact),
        );
        if let Ok(pretty) = serde_json::to_string_pretty(&goal_spec) {
            let _ = std::fs::write(Self::log_path("goal_spec.json"), pretty);
        }
        let endpoint = self
            .config
            .llm_endpoints
            .iter()
            .find(|e| e.role.as_deref() != Some("planner"))
            .unwrap_or(&self.config.llm_endpoints[0]);
        let retry_count = self.config.llm_retry_count;
        let retry_delay = self.config.llm_retry_delay_secs;
        let max_output_lines = self.config.max_output_lines;
        let workspace_listing = capability_pipeline_list_workspace_entries(
            &ctx.cwd[0],
            50,
        );
        let policy = config::CapabilityConfigCapabilityPolicy::snapshot_store_load(
            &ctx.cwd[0],
        )?;
        let policy = config::CapabilityConfigCapabilityPolicy {
            max_node_retries: self.config.max_node_retries,
            ..policy
        };
        let mut store = GraphTemplateStore::new(
            Path::new(TEMPLATE_ROOT).to_path_buf(),
            graph_algo::graph_embedding_dim(),
        );
        let template_name = goal_spec.raw.clone();
        let mut planner_generate = || async {
            let request = decompose::build_goal_decompose_request(
                &goal_spec.raw,
                &ctx.cwd[0],
                &workspace_listing,
                Path::new(LOG_ROOT),
            );
            let mut payload = engine::module_call_llm_json_with_retry_allow_mismatch(
                    &self.bridge,
                    &endpoint.id,
                    &endpoint.url,
                    endpoint.stateful,
                    &request.prompt,
                    "",
                    request.phase,
                    None,
                    &self.tabs,
                    endpoint.max_tabs,
                    self.config.tab_cooldown_ms,
                    self.config.llm_retry_count,
                    self.config.llm_retry_delay_secs,
                )
                .await?;
            let mut extra_retries = 2u32;
            let decomp = loop {
                match decompose::validate_decompose_payload(payload.clone(), &request) {
                    Ok(output) => {
                        decompose::decompose_write_payload_log(
                            &request.log_path,
                            &payload,
                        );
                        break output;
                    }
                    Err(decompose::DecomposeDecomposeRetry::Retry { prompt }) => {
                        if extra_retries == 0 {
                            return Err(
                                anyhow::anyhow!("decompose_goal retries exhausted"),
                            );
                        }
                        extra_retries = extra_retries.saturating_sub(1);
                        payload = engine::module_call_llm_json_with_retry_allow_mismatch(
                                &self.bridge,
                                &endpoint.id,
                                &endpoint.url,
                                endpoint.stateful,
                                &prompt,
                                "",
                                request.phase,
                                None,
                                &self.tabs,
                                endpoint.max_tabs,
                                self.config.tab_cooldown_ms,
                                1,
                                self.config.llm_retry_delay_secs,
                            )
                            .await?;
                    }
                    Err(
                        decompose::DecomposeDecomposeRetry::EnsureRender {
                            prompt,
                            original,
                        },
                    ) => {
                        if extra_retries == 0 {
                            return Err(
                                anyhow::anyhow!("decompose_goal retries exhausted"),
                            );
                        }
                        extra_retries = extra_retries.saturating_sub(1);
                        let retry_payload = engine::module_call_llm_json_with_retry_allow_mismatch(
                                &self.bridge,
                                &endpoint.id,
                                &endpoint.url,
                                endpoint.stateful,
                                &prompt,
                                "",
                                request.phase,
                                None,
                                &self.tabs,
                                endpoint.max_tabs,
                                self.config.tab_cooldown_ms,
                                1,
                                self.config.llm_retry_delay_secs,
                            )
                            .await?;
                        let retry_output = decompose::parse_decompose_payload(
                            retry_payload.clone(),
                        )?;
                        let merged = decompose::decompose_merge_outputs(
                            original,
                            retry_output,
                        );
                        let has_render = merged
                            .tasks
                            .iter()
                            .any(|t| {
                                t.node_type == decompose::DecomposeNodeType::Render
                            });
                        if !has_render {
                            return Err(
                                anyhow::anyhow!(
                                    "decompose_goal retry still missing render node"
                                ),
                            );
                        }
                        decompose::decompose_write_payload_log(
                            &request.log_path,
                            &retry_payload,
                        );
                        break merged;
                    }
                }
            };
            eprintln!("[capability] decompose_goal tasks={}", decomp.tasks.len());
            let mut nodes: Vec<dag::ExecutionNode> = decomp
                .tasks
                .into_iter()
                .map(|t| dag::ExecutionNode {
                    id: t.id,
                    description: t.description,
                    status: dag::NodeStatus::Pending,
                    deps: t.deps,
                    required_capabilities: t.required_capabilities,
                    node_type: t.node_type,
                    priority: t.priority,
                    budget: t.budget,
                    reasoning_trace: t.reasoning_trace,
                    result: None,
                    error: None,
                    readonly_fail_count: 0,
                    repair_attempts: 0,
                    completed_iter: None,
                })
                .collect();
            capability_pipeline_ensure_unique_node_ids(&mut nodes);
            capability_pipeline_ensure_unique_node_ids(&mut nodes);
            if nodes.is_empty() {
                return Err(anyhow::anyhow!("planner_generate produced empty graph"));
            }
            Ok::<
                dag::ExecutionGraph,
                anyhow::Error,
            >(dag::ExecutionGraph {
                nodes,
                id_index: HashMap::new(),
            })
        };
        let mut cache_hit = false;
        let mut resume_loaded = false;
        let mut graph = if self.config.enable_resume {
            let snap = state_snapshot::snapshot_store_load(
                Path::new(&self.config.snapshot_file),
            );
            if let Some(snapshot) = snap {
                if snapshot.goal.raw == goal_spec.raw {
                    telemetry::telemetry_set_resume_iteration(snapshot.iteration);
                    let mut g = snapshot.graph;
                    g.rebuild_index();
                    dag::task_graph_resolve_ready(&mut g);
                    resume_loaded = true;
                    for node in &mut g.nodes {
                        if node.status != dag::NodeStatus::Completed {
                            node.completed_iter = None;
                        }
                    }
                    g
                } else {
                    planner_generate().await?
                }
            } else {
                planner_generate().await?
            }
        } else if store.exists(&template_name) {
            match store.snapshot_store_load(&template_name) {
                Ok(g) if g.validate().is_ok() => {
                    eprintln!("[templates] cache hit");
                    cache_hit = true;
                    g
                }
                _ => {
                    eprintln!("[templates] invalid template, evicting");
                    store.evict(&template_name);
                    let g = planner_generate().await?;
                    let _ = store.snapshot_store_save(&template_name, &g);
                    g
                }
            }
        } else {
            eprintln!("[templates] cache miss — invoking planner");
            let g = planner_generate().await?;
            let _ = store.snapshot_store_save(&template_name, &g);
            g
        };
        if graph.nodes.is_empty() {
            eprintln!("[templates] empty graph; invoking planner");
            graph = planner_generate().await?;
            eprintln!("[templates] planner returned nodes={}", graph.nodes.len());
            cache_hit = false;
            resume_loaded = false;
        }
        graph_analysis_emit_planned_graph(&graph, Path::new(LOG_ROOT), 0);
        graph_analysis_run_graph_algorithms(&graph, Path::new(LOG_ROOT), 0);
        let _ = std::fs::read_to_string(Path::new(LOG_ROOT).join("graph_algorithms.json"));
        if graph_runtime::validate_graph_semantics(&graph, Some(&goal_spec)).is_err() {
            eprintln!("[graph] invalid graph after planning; regenerating");
            graph = planner_generate().await?;
            graph_runtime::validate_graph_semantics(&graph, Some(&goal_spec))
                .map_err(|e| anyhow::anyhow!("graph validation failed: {e}"))?;
        }
        if !self.config.enable_resume || !resume_loaded {
            let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("planner_stage.json"));
        }
        if cache_hit && !self.config.planner_refine_on_cache {
            let mut exec_metrics = Default::default();
            let template_hash = store.hash_for(&template_name);
            let mut failure_store = failure_store::FailureStore::snapshot_store_load(
                &template_hash,
            );
            let mut cost_table = capability_cost::CapabilityCostCapabilityCostTable::snapshot_store_load();
            let (iterations_used, exec_failures) = scheduler::run_execution_loop(
                    &mut graph,
                    &self.bridge,
                    &self.config,
                    &self.role_rr,
                    &self.tabs,
                    &ctx.cwd,
                    &workspace_listing,
                    endpoint,
                    "exec",
                    &policy,
                    self.config.context_radius,
                    self.config.max_concurrency,
                    self.config.max_iterations,
                    self.config.tab_cooldown_ms,
                    retry_count,
                    retry_delay,
                    max_output_lines,
                    0.0,
                    &mut cost_table,
                    &mut exec_metrics,
                    &goal_spec,
                )
                .await?;
            for failure in exec_failures {
                failure_store.record_graph(failure.kind, &graph, failure.iter);
                store.record_failure(&template_hash);
            }
            let features = graph_algo::compute_graph_features_parallel(&graph)
                .with_failure_stats(&failure_store.stats());
            let normalized = graph_algo::graph_analysis_normalize_features(
                &features,
                self.config.max_nodes,
                self.config.max_nodes.saturating_mul(4),
            );
            let policy_outcome = policy_engine::evaluate_policy_normalized(normalized);
            let reward = telemetry::telemetry_compute_reward(
                &graph,
                iterations_used,
                self.config.max_iterations,
                &goal_spec,
            );
            let entry = PolicyTrainingPolicyDatasetEntry {
                features: serde_json::json!(
                    { "nodes" : features.nodes, "edges" : features.edges, "depth" :
                    features.depth, "scc_count" : features.scc_count, "failure_rate" :
                    features.failure_rate, "reward_trend" : features.reward_trend,
                    "avg_out_degree" : features.avg_out_degree, "avg_in_degree" :
                    features.avg_in_degree, "branching_factor" : features
                    .branching_factor, "leaf_count" : features.leaf_count, "root_count" :
                    features.root_count, "verify_to_mutate_ratio" : features
                    .verify_to_mutate_ratio, "observe_to_mutate_ratio" : features
                    .observe_to_mutate_ratio, "node_type_entropy" : features
                    .node_type_entropy, "avg_node_priority" : features.avg_node_priority,
                    "avg_node_budget" : features.avg_node_budget, "blocked_fraction" :
                    features.blocked_fraction, "ready_fraction" : features
                    .ready_fraction, "failed_fraction" : features.failed_fraction,
                    "completion_velocity" : features.completion_velocity, "retry_rate" :
                    features.retry_rate, "failure_pattern_rate" : features
                    .failure_pattern_rate, "cycle_frequency" : features.cycle_frequency,
                    "deadlock_rate" : features.deadlock_rate, "failures" : failure_store
                    .failure_count(), }
                ),
                action: serde_json::json!(
                    { "add_nodes" : 0, "add_edges" : 0, "rewrites" : 0 }
                ),
                policy_decision: serde_json::json!(
                    { "run_planner" : policy_outcome.decision.run_planner,
                    "expansion_scale" : policy_outcome.decision.expansion_scale,
                    "execution_preference" : policy_outcome.decision.execution_preference
                    }
                ),
                reward,
            };
            policy_train::policy_training_append_policy_dataset(&entry);
            policy_train::policy_training_update_online(
                &entry,
                self.config.max_nodes,
                self.config.max_nodes.saturating_mul(4),
            );
            store.record_reward(&template_name, reward);
            let goal_sim = telemetry::telemetry_goal_similarity(&graph, &goal_spec);
            let mut runtime = telemetry::RuntimeTelemetry::default();
            runtime.queue.queue_depth = telemetry::telemetry_pending_requests();
            runtime.queue.retry_rate = 0.0;
            runtime.queue.progress_fraction = telemetry::telemetry_progress_fraction(&graph);
            runtime.queue.iteration_time_ms = 0;
            runtime.queue.branching_factor = features.branching_factor;
            runtime.queue.blocked_fraction = features.blocked_fraction;
            runtime.queue.completion_velocity = features.completion_velocity;
            runtime.queue.deadlock_rate = features.deadlock_rate;
            runtime.policy.policy_prediction = 0.0;
            runtime.policy.policy_error = 0.0;
            runtime.policy.policy_weight_norm = 0.0;
            runtime.policy.dataset_size = 0;
            runtime.policy.policy_run_planner = true;
            runtime.policy.policy_expansion_scale = 1.0;
            runtime.policy.policy_execution_preference = 0.0;
            runtime.template.template_reuse = false;
            runtime.template.template_score = 0.0;
            runtime.template.template_selected = None;
            runtime.template.template_mutations = 0;
            runtime.template.mutation_success_rate = 0.0;
            runtime.template.mutation_reward_delta = 0.0;
            runtime.template.template_reuse_by_embedding = false;
            runtime.template.embedding_cache_hits = 0;
            runtime.repair.repair_attempts = 0;
            runtime.repair.repair_success_rate = 0.0;
            runtime.repair.repair_type = None;
            runtime.repair.constraint_rejections = 0;
            runtime.repair.constraint_hit_rate = 0.0;
            runtime.repair.constraint_types = None;
            runtime.performance.avg_capability_latency = 0.0;
            runtime.performance.avg_capability_failure = 0.0;
            runtime.performance.avg_node_utility = 0.0;
            runtime.snapshot.snapshot_written = false;
            runtime.snapshot.snapshot_loaded = false;
            runtime.snapshot.resume_iteration = 0;
            runtime.goal.goal_similarity_score = goal_sim;
            runtime.goal.goal_drift = (1.0 - goal_sim).clamp(0.0, 1.0);
            runtime.goal.planner_refocus = false;
            let snapshot = telemetry::TelemetryFrame {
                planner: Default::default(),
                exec: exec_metrics.clone(),
                runtime,
                reward,
                template_hash: Some(store.hash_for(&template_name)),
                goal: Some(template_name.clone()),
            };
            telemetry::telemetry_record_snapshot(
                &Path::new(LOG_ROOT).join("metrics.json"),
                &snapshot,
            );
            telemetry::telemetry_record_snapshot(
                &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                &snapshot,
            );
            let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
            telemetry::telemetry_record_snapshot(
                &Path::new(TEMPLATE_ROOT)
                    .join(format!("metrics_{}.json", template_hash)),
                &snapshot,
            );
            if let Ok(text) = std::fs::read_to_string(Path::new(LOG_ROOT).join("metrics.json")) {
                eprintln!("[logs] capability_metrics {}", text.trim());
            }
            Ok(reward)
        } else {
            let planner_endpoint = self.config.planner_endpoint()?;
            let mut planner_session = planner_session::PlannerController::new(
                planner_endpoint,
                goal_spec.clone(),
            );
            let recent = store.recent_rewards(&template_name, 4);
            let plateaued = store
                .is_plateaued(
                    &template_name,
                    self.config.planner_plateau_window,
                    self.config.planner_plateau_threshold,
                );
            let similar = store
                .find_similar(
                    &goal_spec,
                    &graph,
                    1,
                    self.config.goal_similarity_weight,
                    self.config.structural_similarity_weight,
                    self.config.template_failure_hard_ban,
                );
            let bootstrap_seed = similar
                .templates
                .into_iter()
                .next()
                .map(|s| {
                    let seed_graph = store.snapshot_store_load(&s.entry.goal).ok();
                    let node_summaries = seed_graph
                        .as_ref()
                        .map(|g| {
                            g.nodes
                                .iter()
                                .map(|n| {
                                    let desc = if n.description.len() > 60 {
                                        format!("{}…", &n.description[..60])
                                    } else {
                                        n.description.clone()
                                    };
                                    format!("{}: {}", n.id, desc)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    planner_session::PlannerControllerBootstrapSeed {
                        goal: s.entry.goal.clone(),
                        similarity_score: s.score,
                        reward: s.entry.reward,
                        node_summaries,
                        capability_set: s.entry.capability_set.clone(),
                        node_count: s.entry.node_count,
                        edge_count: s.entry.edge_count,
                    }
                });
            let reward_ctx = planner_session::PlannerControllerRewardContext {
                recent_rewards: recent,
                plateaued,
                best_reward: store.stored_reward(&template_name),
                stored_reward: store.stored_reward(&template_name),
                bootstrap_seed,
            };
            planner_session.set_reward_context(reward_ctx);
            let planner_stage_path = Path::new(LOG_ROOT).join("planner_stage.json");
            let start_stage = PlannerStagePersist::load(&planner_stage_path)
                .map(|persist| persist.stage)
                .unwrap_or(PlannerStage::ReuseTemplate);
            let prev_reward = store.stored_reward(&template_name);
            let reward = scheduler::run_planner_loop(
                    &mut planner_session,
                    &mut graph,
                    &self.bridge,
                    &self.config,
                    &self.role_rr,
                    &self.tabs,
                    &ctx.cwd,
                    &workspace_listing,
                    planner_endpoint,
                    "exec",
                    &policy,
                    self.config.context_radius,
                    self.config.max_concurrency,
                    self.config.max_iterations,
                    self.config.tab_cooldown_ms,
                    retry_count,
                    retry_delay,
                    max_output_lines,
                    &mut store,
                    &template_name,
                    start_stage,
                    Some(planner_stage_path.as_path()),
                    ctx.tick,
                )
                .await?;
            let completion_velocity = if reward > prev_reward { 1.0 } else { 0.0 };
            let goal_sim = telemetry::telemetry_goal_similarity(&graph, &goal_spec);
            let mut runtime = telemetry::RuntimeTelemetry::default();
            runtime.queue.queue_depth = telemetry::telemetry_pending_requests();
            runtime.queue.progress_fraction = telemetry::telemetry_progress_fraction(&graph);
            runtime.queue.completion_velocity = completion_velocity;
            runtime.policy.policy_run_planner = true;
            runtime.goal.goal_similarity_score = goal_sim;
            runtime.goal.goal_drift = (1.0 - goal_sim).clamp(0.0, 1.0);
            runtime.goal.planner_refocus = false;
            let snapshot = telemetry::TelemetryFrame {
                planner: Default::default(),
                exec: Default::default(),
                runtime,
                reward,
                template_hash: Some(store.hash_for(&template_name)),
                goal: Some(template_name.clone()),
            };
            telemetry::telemetry_record_snapshot(
                &Path::new(LOG_ROOT).join("metrics.json"),
                &snapshot,
            );
            telemetry::telemetry_record_snapshot(
                &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                &snapshot,
            );
            if let Ok(text) = std::fs::read_to_string(Path::new(LOG_ROOT).join("metrics.json")) {
                eprintln!("[logs] capability_metrics {}", text.trim());
            }
            Ok(reward)
        }
    }
}
fn capability_pipeline_list_workspace_entries(root: &Path, limit: usize) -> String {
    let mut entries: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries.truncate(limit);
    entries.join(", ")
}
pub(crate) fn capability_pipeline_ensure_unique_node_ids(
    nodes: &mut Vec<dag::ExecutionNode>,
) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in nodes.iter_mut() {
        let count = counts.entry(n.id.clone()).or_insert(0);
        if *count > 0 {
            let new_id = format!("{}__{}", n.id, * count);
            n.id = new_id;
        }
        *count += 1;
    }
}
#[async_trait::async_trait]
impl Pipeline for CapabilityPipeline {
    fn name(&self) -> &str {
        "capability"
    }
    async fn capability_pipeline_pipeline_run_tick(
        &self,
        ctx: &PipelineContext,
        _ir: &mut SystemState,
        _layout: &mut FileTopology,
    ) -> Result<PipelineOutcome> {
        match self.run_capability_loop(ctx).await {
            Ok(reward) => {
                Ok(PipelineOutcome {
                    reward,
                    summary: "capability completed".into(),
                    advanced: true,
                    stage: crate::ir::PipelineStage::Act,
                })
            }
            Err(e) => {
                Ok(PipelineOutcome {
                    reward: -1.0,
                    summary: format!("capability error: {e}"),
                    advanced: false,
                    stage: crate::ir::PipelineStage::Observe,
                })
            }
        }
    }
}
