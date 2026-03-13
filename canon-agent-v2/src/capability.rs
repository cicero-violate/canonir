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
use crate::tlog;
use graph_algo::{graph_analysis_emit_planned_graph, graph_analysis_run_graph_algorithms};
use policy::ExecutionPolicyModel;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use templates::GraphTemplateStore;
pub const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";
pub const TEMPLATE_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/templates";
#[derive(Clone, Default)]
struct TemplateMutationStats {
    add_nodes: u64,
    add_edges: u64,
    rewrites: u64,
    reward_delta: f64,
    stage_reuse: u64,
    stage_mutate: u64,
    stage_patch: u64,
    stage_execute: u64,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionDelta {
    ReadFile { path: String },
    ListDir { path: String },
    ReadCommand {
        command: String,
        args: Vec<String>,
        #[serde(default)]
        path: Option<String>,
    },
    WriteFile { path: String, content: String },
    ReplaceText { path: String, find: String, replace: String },
    DeleteFile { path: String },
}
pub struct CapabilityPipeline {
    bridge: WsBridge,
    config: CapabilityConfig,
    tabs: engine::TabManagerHandle,
    role_rr: std::sync::Arc<tokio::sync::Mutex<HashMap<String, usize>>>,
}
impl CapabilityPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = CapabilityConfig::snapshot_store_load()
            .expect("failed to load capability config");
        config.apply_env_flags();
        Self {
            bridge,
            config,
            tabs: engine::llm_worker_new_tabs(),
            role_rr: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
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
    fn clear_capability_log_dir() {
        if let Ok(entries) = std::fs::read_dir(LOG_ROOT) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
            }
        }
        let _ = std::fs::create_dir_all(LOG_ROOT);
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
        Self::clear_capability_log_dir();
        if self.config.llm_endpoints.is_empty() {
            anyhow::bail!("capability config has no llm endpoints");
        }
        engine::module_init_io_workers(&self.bridge, &self.config, &self.tabs).await;
        let mut goal = CapabilityConfigGoalSpec::from_file(&self.config.goal_file)?;
        let intent_path = Path::new(
            "/workspace/ai_sandbox/canon/state/intent_state.json",
        );
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
        let selection = objectives::load_goal_from_reports(
            objectives::ObjectiveWeights::default(),
        );
        if let Some(selection) = selection.as_ref() {
            goal.raw = objectives::goal_raw_with_artifact(
                &goal.raw,
                &selection.artifact,
            );
        }
        let goal_spec = GoalSpec::new_with_artifact(
            goal.raw.clone(),
            graph_algo::graph_embedding_dim(),
            selection.map(|s| s.artifact),
        );
        tlog::emit(
            "goal_update",
            serde_json::json!({
                "goal": goal_spec.raw,
                "artifact": goal_spec.artifact,
            }),
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
        let goal_raw = std::sync::Arc::new(goal_spec.raw.clone());
        let cwd0 = std::sync::Arc::new(ctx.cwd[0].clone());
        let workspace_listing = workspace_listing.clone();
        let workspace_listing_arc = std::sync::Arc::new(workspace_listing.clone());
        let bridge = self.bridge.clone();
        let tabs = self.tabs.clone();
        let endpoint_id = endpoint.id.clone();
        let endpoint_url = endpoint.url.clone();
        let endpoint_stateful = endpoint.stateful;
        let endpoint_max_tabs = endpoint.max_tabs;
        let tab_cooldown_ms = self.config.tab_cooldown_ms;
        let llm_retry_count = self.config.llm_retry_count;
        let llm_retry_delay_secs = self.config.llm_retry_delay_secs;
        let planner_generate: crate::async_pipeline::PlannerGenerateFn =
            std::sync::Arc::new(move || {
                let goal_raw = goal_raw.clone();
                let cwd0 = cwd0.clone();
                let workspace_listing = workspace_listing_arc.clone();
                let bridge = bridge.clone();
                let tabs = tabs.clone();
                let endpoint_id = endpoint_id.clone();
                let endpoint_url = endpoint_url.clone();
                Box::pin(async move {
                    let request = decompose::build_goal_decompose_request(
                        &goal_raw,
                        &cwd0,
                        &workspace_listing,
                        Path::new(LOG_ROOT),
                    );
                    eprintln!(
                        "[planner] decompose_send endpoint={} chars={}",
                        endpoint_id,
                        request.prompt.len()
                    );
                    let mut payload = engine::module_call_llm_json_with_retry_allow_mismatch(
                            &bridge,
                            &endpoint_id,
                            &endpoint_url,
                            endpoint_stateful,
                            &request.prompt,
                            "",
                            request.phase,
                            None,
                            &tabs,
                            endpoint_max_tabs,
                            tab_cooldown_ms,
                            llm_retry_count,
                            llm_retry_delay_secs,
                        )
                        .await?;
                    eprintln!("[planner] decompose_recv type=Value");
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
                                &bridge,
                                &endpoint_id,
                                &endpoint_url,
                                endpoint_stateful,
                                &prompt,
                                "",
                                request.phase,
                                None,
                                &tabs,
                                endpoint_max_tabs,
                                tab_cooldown_ms,
                                1,
                                llm_retry_delay_secs,
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
                        let retry_payload = engine::module_call_llm_json_with_retry_allow_mismatch(
                                &bridge,
                                &endpoint_id,
                                &endpoint_url,
                                endpoint_stateful,
                                &prompt,
                                "",
                                request.phase,
                                None,
                                &tabs,
                                endpoint_max_tabs,
                                tab_cooldown_ms,
                                1,
                                llm_retry_delay_secs,
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
                })
            });
        let planner_generate_once = || async {
            (planner_generate.as_ref())().await
        };
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
                    planner_generate_once().await?
                }
            } else {
                planner_generate_once().await?
            }
        } else if store.template_exists(&template_name) {
            match store.load_snapshot(&template_name) {
                Ok(g) if g.validate().is_ok() => {
                    eprintln!("[templates] cache hit");
                    let mut g = g;
                    for n in &mut g.nodes {
                        n.status = dag::NodeStatus::Pending;
                        n.result = None;
                        n.error = None;
                        n.readonly_fail_count = 0;
                        n.repair_attempts = 0;
                        n.completed_iter = None;
                    }
                    g.rebuild_index();
                    dag::task_graph_resolve_ready(&mut g);
                    g
                }
                _ => {
                    eprintln!("[templates] invalid template, evicting");
                    store.evict(&template_name);
                    let g = planner_generate_once().await?;
                    let _ = store.save_snapshot(&template_name, &g);
                    g
                }
            }
        } else {
            eprintln!("[templates] cache miss — invoking planner");
            let g = planner_generate_once().await?;
            let _ = store.save_snapshot(&template_name, &g);
            g
        };
        if graph.nodes.is_empty() {
            eprintln!("[templates] empty graph; invoking planner");
            graph = planner_generate_once().await?;
            eprintln!("[templates] planner returned nodes={}", graph.nodes.len());
            resume_loaded = false;
        }
        if !resume_loaded {
            for node in &graph.nodes {
                tlog::emit(
                    "task_created",
                    serde_json::json!({
                        "id": node.id,
                        "description": node.description,
                        "deps": node.deps,
                        "node_type": node.node_type,
                        "capabilities": node.required_capabilities,
                        "priority": node.priority,
                        "budget": node.budget,
                    }),
                );
            }
        }
        graph_analysis_emit_planned_graph(&graph, Path::new(LOG_ROOT), 0);
        graph_analysis_run_graph_algorithms(&graph, Path::new(LOG_ROOT), 0);
        let _ = std::fs::read_to_string(
            Path::new(LOG_ROOT).join("graph_algorithms.json"),
        );
        if graph_runtime::ensure_render_reachable(&mut graph) {
            eprintln!("[graph] repaired render reachability after planning");
        }
        if graph_runtime::must_validate_graph_semantics(&graph, Some(&goal_spec))
            .is_err()
        {
            eprintln!("[graph] invalid graph after planning; regenerating");
            graph = planner_generate_once().await?;
            if graph_runtime::ensure_render_reachable(&mut graph) {
                eprintln!("[graph] repaired render reachability after regeneration");
            }
            graph_runtime::must_validate_graph_semantics(&graph, Some(&goal_spec))
                .map_err(|e| anyhow::anyhow!("graph validation failed: {e}"))?;
        }
        if !self.config.enable_resume || !resume_loaded {
            let _ = std::fs::remove_file(Path::new(LOG_ROOT).join("planner_stage.json"));
        }
        // Async pipeline only (legacy sequential path removed).
        let planner_endpoint = self.config.planner_endpoint()?;
        let mut planner_session = planner_session::PlannerController::new(
            planner_endpoint,
            goal_spec.clone(),
        );
        let recent = store.recent_template_rewards(&template_name, 4);
        let plateaued = store
            .is_reward_plateaued(
                &template_name,
                self.config.planner_plateau_window,
                self.config.planner_plateau_threshold,
            );
        let similar = store
            .find_similar_templates(
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
                let seed_graph = store.load_snapshot(&s.entry.goal).ok();
                let node_summaries = seed_graph
                    .as_ref()
                    .map(|g| {
                        g.nodes
                            .iter()
                            .map(|n| {
                                let desc = if n.description.len() > 60 {
                                    format!("{}…", & n.description[..60])
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
            best_reward: store.reward_for_template(&template_name),
            stored_reward: store.reward_for_template(&template_name),
            bootstrap_seed,
        };
        planner_session.set_reward_context(reward_ctx);
        let planner_session = std::sync::Arc::new(tokio::sync::Mutex::new(planner_session));
        let store = std::sync::Arc::new(tokio::sync::Mutex::new(store));
        let planner_stage_path = Path::new(LOG_ROOT).join("planner_stage.json");
        let template_name_async = template_name.clone();
        let config = std::sync::Arc::new(self.config.clone());
        let policy_arc = std::sync::Arc::new(policy.clone());
        let policy_for_task = policy_arc.clone();
        let role_rr = self.role_rr.clone();
        let bridge = self.bridge.clone();
        let tabs = self.tabs.clone();
        let cwd = ctx.cwd.clone();
        let workspace_listing_async = workspace_listing.clone();
        let planner_endpoint_async = planner_endpoint.clone();
        let store_for_planner = store.clone();
        let mutation_stats = std::sync::Arc::new(tokio::sync::Mutex::new(TemplateMutationStats::default()));
        let mutation_stats_planner = mutation_stats.clone();
        let planner_task: crate::async_pipeline::PlannerTaskFn =
            std::sync::Arc::new(move |graph, tick| {
                let planner_session = planner_session.clone();
                let store = store_for_planner.clone();
                let config = config.clone();
                let policy = policy_for_task.clone();
                let role_rr = role_rr.clone();
                let bridge = bridge.clone();
                let tabs = tabs.clone();
                let cwd = cwd.clone();
                let workspace_listing_async = workspace_listing_async.clone();
                let planner_endpoint_async = planner_endpoint_async.clone();
                let planner_stage_path = planner_stage_path.clone();
                let template_name_async = template_name_async.clone();
                let mutation_stats = mutation_stats_planner.clone();
                Box::pin(async move {
                    let start_stage = PlannerStagePersist::load(&planner_stage_path)
                        .map(|persist| persist.stage)
                        .unwrap_or(PlannerStage::ReuseTemplate);
                    {
                        let mut stats = mutation_stats.blocking_lock();
                        *stats = TemplateMutationStats::default();
                    }
                    let mut session = planner_session.lock().await;
                    let mut store = store.lock().await;
                    let mut g = graph.lock().await;
                    let before_nodes = g.nodes.len() as u64;
                    let before_edges = graph_algo::graph_analysis_edge_count(&g) as u64;
                    let before_reward = telemetry::telemetry_compute_reward(
                        &g,
                        0,
                        config.max_iterations,
                        session.goal_spec(),
                    );
                    let mut before_desc = HashMap::new();
                    for n in &g.nodes {
                        before_desc.insert(n.id.clone(), n.description.clone());
                    }
                    scheduler::run_planner_loop(
                        &mut session,
                        &mut g,
                        &bridge,
                        config.as_ref(),
                        &role_rr,
                        &tabs,
                        &cwd,
                        &workspace_listing_async,
                        &planner_endpoint_async,
                        "exec",
                        policy.as_ref(),
                        config.context_radius,
                        config.max_concurrency,
                        config.max_iterations,
                        config.tab_cooldown_ms,
                        retry_count,
                        retry_delay,
                        max_output_lines,
                        &mut store,
                        &template_name_async,
                        start_stage,
                        Some(planner_stage_path.as_path()),
                        tick,
                    )
                    .await
                    .map(|reward| {
                        {
                            let mut stats = mutation_stats.blocking_lock();
                            match start_stage {
                                PlannerStage::ReuseTemplate => stats.stage_reuse = stats.stage_reuse.saturating_add(1),
                                PlannerStage::MutateTemplate => stats.stage_mutate = stats.stage_mutate.saturating_add(1),
                                PlannerStage::GraphPatch => stats.stage_patch = stats.stage_patch.saturating_add(1),
                                PlannerStage::Execute => stats.stage_execute = stats.stage_execute.saturating_add(1),
                                _ => {}
                            }
                        }
                        let after_nodes = g.nodes.len() as u64;
                        let after_edges = graph_algo::graph_analysis_edge_count(&g) as u64;
                        let mut rewrites = 0u64;
                        for n in &g.nodes {
                            if let Some(prev) = before_desc.get(&n.id) {
                                if prev != &n.description {
                                    rewrites += 1;
                                }
                            }
                        }
                        let add_nodes = after_nodes.saturating_sub(before_nodes);
                        let add_edges = after_edges.saturating_sub(before_edges);
                        let reward_delta = reward - before_reward;
                        let mut stats = mutation_stats.blocking_lock();
                        stats.add_nodes = add_nodes;
                        stats.add_edges = add_edges;
                        stats.rewrites = rewrites;
                        stats.reward_delta = reward_delta;
                        // per-planner-iteration telemetry
                        let features = graph_algo::compute_graph_features_parallel(&g);
                        let goal_sim = telemetry::telemetry_goal_similarity(&g, session.goal_spec());
                        let mut runtime = telemetry::RuntimeTelemetry::default();
                        runtime.queue.queue_depth = telemetry::telemetry_pending_requests();
                        runtime.queue.progress_fraction = telemetry::telemetry_progress_fraction(&g);
                        runtime.queue.branching_factor = features.branching_factor;
                        runtime.queue.blocked_fraction = features.blocked_fraction;
                        runtime.queue.completion_velocity = features.completion_velocity;
                        runtime.queue.deadlock_rate = features.deadlock_rate;
                        runtime.goal.goal_similarity_score = goal_sim;
                        runtime.goal.goal_drift = (1.0 - goal_sim).clamp(0.0, 1.0);
                        runtime.goal.planner_refocus = false;
                        let template_hash = store.template_hash(&template_name_async);
                        let mut tmpl = telemetry::RuntimeTemplateTelemetry::default();
                        tmpl.template_reuse = store.template_exists(&template_name_async);
                        tmpl.template_score = store.reward_for_template(&template_name_async);
                        tmpl.template_selected = Some(template_name_async.clone());
                        tmpl.template_mutations = add_nodes + add_edges + rewrites;
                        tmpl.template_new_nodes = add_nodes;
                        tmpl.template_new_edges = add_edges;
                        tmpl.template_rewrites = rewrites;
                        {
                            let stats = mutation_stats.blocking_lock();
                            tmpl.template_mutations_reuse = stats.stage_reuse;
                            tmpl.template_mutations_mutate = stats.stage_mutate;
                            tmpl.template_mutations_patch = stats.stage_patch;
                            tmpl.template_mutations_execute = stats.stage_execute;
                        }
                        tmpl.mutation_success_rate = if tmpl.template_mutations == 0 { 0.0 } else { 1.0 };
                        tmpl.mutation_reward_delta = reward_delta;
                        runtime.template = tmpl;
                        let snapshot = telemetry::TelemetryFrame {
                            planner: Default::default(),
                            exec: Default::default(),
                            runtime,
                            reward,
                            template_hash: Some(template_hash),
                            goal: Some(template_name_async.clone()),
                        };
                        telemetry::telemetry_record_snapshot(&Path::new(LOG_ROOT).join("metrics.json"), &snapshot);
                        telemetry::telemetry_record_snapshot(
                            &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                            &snapshot,
                        );
                        reward
                    })
                })
            });
        let template_store = store.clone();
        let template_name_hook = template_name.clone();
        let template_reward: crate::async_pipeline::TemplateRewardHook =
            std::sync::Arc::new(move |reward| {
                let template_store = template_store.clone();
                let template_name_hook = template_name_hook.clone();
                Box::pin(async move {
                    let store = template_store.lock().await;
                    store.record_template_reward(&template_name_hook, reward);
                    Ok(())
                })
            });
        let template_store = store.clone();
        let template_name_hook = template_name.clone();
        let template_failure: crate::async_pipeline::TemplateFailureHook =
            std::sync::Arc::new(move |_reason| {
                let template_store = template_store.clone();
                let template_name_hook = template_name_hook.clone();
                Box::pin(async move {
                    let mut store = template_store.lock().await;
                    let hash = store.template_hash(&template_name_hook);
                    store.record_template_failure(&hash);
                    Ok(())
                })
            });
        let template_store = store.clone();
        let template_name_hook = template_name.clone();
        let template_telemetry: crate::async_pipeline::TemplateTelemetryHook =
            std::sync::Arc::new(move || {
                let template_store = template_store.clone();
                let template_name_hook = template_name_hook.clone();
                let mutation_stats = mutation_stats.clone();
                Box::pin(async move {
                    let store = template_store.lock().await;
                    let hash = store.template_hash(&template_name_hook);
                    let mut rt = telemetry::RuntimeTemplateTelemetry::default();
                    let stats = mutation_stats.lock().await.clone();
                    rt.template_reuse = store.template_exists(&template_name_hook);
                    rt.template_score = store.reward_for_template(&template_name_hook);
                    rt.template_selected = Some(template_name_hook.clone());
                    rt.template_mutations = stats.add_nodes + stats.add_edges + stats.rewrites;
                    rt.template_new_nodes = stats.add_nodes;
                    rt.template_new_edges = stats.add_edges;
                    rt.template_rewrites = stats.rewrites;
                    rt.template_mutations_reuse = stats.stage_reuse;
                    rt.template_mutations_mutate = stats.stage_mutate;
                    rt.template_mutations_patch = stats.stage_patch;
                    rt.template_mutations_execute = stats.stage_execute;
                    rt.mutation_success_rate = if rt.template_mutations == 0 { 0.0 } else { 1.0 };
                    rt.mutation_reward_delta = stats.reward_delta;
                    rt.template_reuse_by_embedding = false;
                    rt.embedding_cache_hits = 0;
                    rt.objective_delta = 0.0;
                    rt.template_hit_rate = 0.0;
                    Ok((Some(hash), rt))
                })
            });
        return crate::async_pipeline::run_async_pipeline(
            graph,
            planner_generate.clone(),
            Some(planner_task),
            Some(template_reward),
            Some(template_failure),
            Some(template_telemetry),
            self.bridge.clone(),
            std::sync::Arc::new(self.config.clone()),
            self.role_rr.clone(),
            self.tabs.clone(),
            &ctx.cwd,
            &workspace_listing,
            endpoint.clone(),
            "exec",
            policy_arc.clone(),
            self.config.context_radius,
            self.config.max_concurrency,
            self.config.max_iterations,
            self.config.tab_cooldown_ms,
            retry_count,
            retry_delay,
            max_output_lines,
            &goal_spec,
            Path::new(LOG_ROOT),
        )
        .await;
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
