//! Capability-driven DAG pipeline.

pub mod capability;
pub mod dag;
pub mod config;
pub mod graph_algo;
pub mod graph_runtime;
pub mod graph_maintenance;
pub mod executor_dispatch;
pub mod endpoint_scheduler;
pub mod endpoint_worker;
pub mod response_router;
pub mod scheduler;
pub mod scheduler_state;
pub mod scheduler_scoring;
pub mod dispatch;
pub mod execution_result;
pub mod planner_session;
pub mod planner_state;
pub mod telemetry;
pub mod llm;
pub mod decompose;
pub mod engine;
pub mod act;
pub mod tab_management;
pub mod console;
pub mod templates;
pub mod template_index;
pub mod failure_store;
pub mod policy;
pub mod policy_engine;
pub mod policy_train;
pub mod gpu_scheduler;
pub mod capability_cost;
pub mod template_mutation;
pub mod state_snapshot;
pub mod goal_embedding;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::{CapabilityConfig, GoalSpec};
use graph_algo::{emit_planned_graph, run_graph_algorithms};
use llm::call_agent_json_with_retry_allow_mismatch;
use templates::TemplateStore;
use policy_train::PolicyDatasetEntry;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use policy::PolicyModel;

pub(crate) const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";
pub(crate) const TEMPLATE_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/templates";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    ReadFile { path: String },
    ListDir { path: String },
    ReadCommand { command: String, args: Vec<String> },
    WriteFile { path: String, content: String },
    ReplaceText { path: String, find: String, replace: String },
    DeleteFile { path: String },
}

pub struct CapabilityPipeline {
    bridge: WsBridge,
    config: CapabilityConfig,
    tabs: tab_management::TabsHandle,
    role_rr: tokio::sync::Mutex<HashMap<String, usize>>,
}

impl CapabilityPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = CapabilityConfig::load().expect("failed to load capability config");
        Self {
            bridge,
            config,
            tabs: Arc::new(tokio::sync::Mutex::new(tab_management::TabSlots::new())),
            role_rr: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    fn ensure_log_dir() {
        let _ = std::fs::create_dir_all(LOG_ROOT);
        let _ = std::fs::create_dir_all("/workspace/ai_sandbox/canon/agent_logs");
        let _ = std::fs::create_dir_all("/workspace/ai_sandbox/canon/agent_logs/templates");
        Self::ensure_agent_log_files();
    }

    fn ensure_agent_log_files() {
        Self::ensure_file("/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl", "");
        Self::ensure_file("/workspace/ai_sandbox/canon/agent_logs/goal_embeddings.json", "{}");
        Self::ensure_file("/workspace/ai_sandbox/canon/agent_logs/metrics.json", "{}");
        Self::ensure_file("/workspace/ai_sandbox/canon/agent_logs/capability_costs.json", "{}");
        let weights_path = Path::new("/workspace/ai_sandbox/canon/agent_logs/policy_weights.json");
        if !weights_path.exists() {
            let model = PolicyModel::load_default();
            let _ = model.save(weights_path);
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
        endpoint_worker::init_workers(&self.bridge, &self.config, &self.tabs).await;

        let goal = GoalSpec::from_file(&self.config.goal_file)?;
        if let Ok(pretty) = serde_json::to_string_pretty(&goal) {
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
        let workspace_listing = list_workspace_entries(&ctx.cwd[0], 50);
        let policy = config::CapabilityPolicy::load(&ctx.cwd[0])?;
        let policy = config::CapabilityPolicy { max_node_retries: self.config.max_node_retries, ..policy };

        let mut store = TemplateStore::new(Path::new(TEMPLATE_ROOT).to_path_buf(), self.config.embedding_dim);
        let template_name = goal.raw.clone();

        let mut planner_generate = || async {
            let request = decompose::build_goal_request(
                &goal.raw,
                &ctx.cwd[0],
                &workspace_listing,
                Path::new(LOG_ROOT),
            );
            let mut payload = call_agent_json_with_retry_allow_mismatch(
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
                match decompose::evaluate_payload(payload.clone(), &request) {
                    Ok(output) => {
                        decompose::write_payload_log(&request.log_path, &payload);
                        break output;
                    }
                    Err(decompose::DecomposeRetry::Retry { prompt }) => {
                        if extra_retries == 0 {
                            return Err(anyhow::anyhow!("decompose_goal retries exhausted"));
                        }
                        extra_retries = extra_retries.saturating_sub(1);
                        payload = call_agent_json_with_retry_allow_mismatch(
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
                    Err(decompose::DecomposeRetry::EnsureRender { prompt, original }) => {
                        if extra_retries == 0 {
                            return Err(anyhow::anyhow!("decompose_goal retries exhausted"));
                        }
                        extra_retries = extra_retries.saturating_sub(1);
                        let retry_payload = call_agent_json_with_retry_allow_mismatch(
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
                        let retry_output = decompose::parse_payload(retry_payload.clone())?;
                        let merged = decompose::merge_outputs(original, retry_output);
                        let has_render = merged.tasks.iter().any(|t| t.node_type == decompose::NodeType::Render);
                        if !has_render {
                            return Err(anyhow::anyhow!("decompose_goal retry still missing render node"));
                        }
                        decompose::write_payload_log(&request.log_path, &retry_payload);
                        break merged;
                    }
                }
            };
            eprintln!("[capability] decompose_goal tasks={}", decomp.tasks.len());

        let mut nodes: Vec<dag::TaskNode> = decomp.tasks.into_iter().map(|t| dag::TaskNode {
            id: t.id,
            description: t.description,
            status: dag::Status::Pending,
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
        }).collect();
            ensure_unique_node_ids(&mut nodes);
            ensure_unique_node_ids(&mut nodes);
            Ok::<dag::TaskGraph, anyhow::Error>(dag::TaskGraph { nodes, id_index: HashMap::new() })
        };

        let mut cache_hit = false;
        let mut graph = if self.config.enable_resume {
            let snap = state_snapshot::load(Path::new(&self.config.snapshot_file));
            if let Some(snapshot) = snap {
                telemetry::set_resume_iteration(snapshot.iteration);
                let mut g = snapshot.graph;
                g.reset_for_execution();
                dag::resolve_ready(&mut g);
                g
            } else {
                planner_generate().await?
            }
        } else if store.exists(&template_name) {
            match store.load(&template_name) {
                Ok(g) if g.validate().is_ok() => {
                    eprintln!("[templates] cache hit");
                    cache_hit = true;
                    g
                }
                _ => {
                    eprintln!("[templates] invalid template, evicting");
                    store.evict(&template_name);
                    let g = planner_generate().await?;
                    let _ = store.save(&template_name, &g);
                    g
                }
            }
        } else {
            eprintln!("[templates] cache miss — invoking planner");
            let g = planner_generate().await?;
            let _ = store.save(&template_name, &g);
            g
        };

        emit_planned_graph(&graph, Path::new(LOG_ROOT), 0);
        run_graph_algorithms(&graph, Path::new(LOG_ROOT), 0);

        if cache_hit && !self.config.planner_refine_on_cache {
            let mut exec_metrics = Default::default();
            let template_hash = store.hash_for(&template_name);
            let mut failure_store = failure_store::FailureStore::load(&template_hash);
            let mut cost_table = capability_cost::CapabilityCostTable::load();
            let (iterations_used, exec_failures) = scheduler::execute_graph_loop(
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
            )
            .await?;
            for failure in exec_failures {
                failure_store.record_graph(failure.kind, &graph, failure.iter);
                store.record_failure(&template_hash);
            }
            let features = graph_algo::graph_features(&graph).with_failure_stats(&failure_store.stats());
            let policy_outcome = policy_engine::evaluate(&features, &self.config);
            let reward = telemetry::compute_reward(&graph, iterations_used, self.config.max_iterations);
            let entry = PolicyDatasetEntry {
                features: serde_json::json!({
                    "nodes": features.nodes,
                    "edges": features.edges,
                    "depth": features.depth,
                    "scc_count": features.scc_count,
                    "failure_rate": features.failure_rate,
                    "reward_trend": features.reward_trend,
                    "avg_out_degree": features.avg_out_degree,
                    "avg_in_degree": features.avg_in_degree,
                    "branching_factor": features.branching_factor,
                    "leaf_count": features.leaf_count,
                    "root_count": features.root_count,
                    "verify_to_mutate_ratio": features.verify_to_mutate_ratio,
                    "observe_to_mutate_ratio": features.observe_to_mutate_ratio,
                    "node_type_entropy": features.node_type_entropy,
                    "avg_node_priority": features.avg_node_priority,
                    "avg_node_budget": features.avg_node_budget,
                    "blocked_fraction": features.blocked_fraction,
                    "ready_fraction": features.ready_fraction,
                    "failed_fraction": features.failed_fraction,
                    "completion_velocity": features.completion_velocity,
                    "retry_rate": features.retry_rate,
                    "failure_pattern_rate": features.failure_pattern_rate,
                    "cycle_frequency": features.cycle_frequency,
                    "deadlock_rate": features.deadlock_rate,
                    "failures": failure_store.failure_count(),
                }),
                action: serde_json::json!({
                    "add_nodes": 0,
                    "add_edges": 0,
                    "rewrites": 0
                }),
                policy_decision: serde_json::json!({
                    "run_planner": policy_outcome.decision.run_planner,
                    "expansion_scale": policy_outcome.decision.expansion_scale,
                    "execution_preference": policy_outcome.decision.execution_preference
                }),
                reward,
            };
            policy_train::append_policy_dataset(&entry);
            policy_train::update_online(&entry, self.config.max_nodes, self.config.max_nodes.saturating_mul(4));
            store.record_reward(&template_name, reward);
            let runtime = telemetry::RuntimeMetrics {
                queue_depth: telemetry::pending_requests(),
                retry_rate: 0.0,
                progress_fraction: telemetry::progress_fraction(&graph),
                iteration_time_ms: 0,
                branching_factor: features.branching_factor,
                blocked_fraction: features.blocked_fraction,
                completion_velocity: features.completion_velocity,
                policy_prediction: 0.0,
                policy_error: 0.0,
                policy_weight_norm: 0.0,
                dataset_size: 0,
                deadlock_rate: features.deadlock_rate,
                policy_run_planner: true,
                policy_expansion_scale: 1.0,
                policy_execution_preference: 0.0,
                template_reuse: false,
                template_score: 0.0,
                template_selected: None,
                repair_attempts: 0,
                repair_success_rate: 0.0,
                repair_type: None,
                constraint_rejections: 0,
                constraint_hit_rate: 0.0,
                constraint_types: None,
                avg_capability_latency: 0.0,
                avg_capability_failure: 0.0,
                avg_node_utility: 0.0,
                template_mutations: 0,
                mutation_success_rate: 0.0,
                mutation_reward_delta: 0.0,
                snapshot_written: false,
                snapshot_loaded: false,
                resume_iteration: 0,
                goal_similarity_score: 0.0,
                template_reuse_by_embedding: false,
                embedding_cache_hits: 0,
            };
            let snapshot = telemetry::TelemetrySnapshot {
                planner: Default::default(),
                exec: exec_metrics.clone(),
                runtime,
                reward,
                template_hash: Some(store.hash_for(&template_name)),
                goal: Some(template_name.clone()),
            };
            telemetry::record_snapshot(&Path::new(LOG_ROOT).join("metrics.json"), &snapshot);
            telemetry::record_snapshot(
                &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                &snapshot,
            );
            let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
            telemetry::record_snapshot(
                &Path::new(TEMPLATE_ROOT).join(format!("metrics_{}.json", template_hash)),
                &snapshot,
            );
            Ok(reward)
        } else {
            let planner_endpoint = self.config.planner_endpoint()?;
            let mut planner_session = planner_session::PlannerSession::new(planner_endpoint, goal.raw.clone());
            let recent = store.recent_rewards(&template_name, 4);
            let plateaued = store.is_plateaued(
                &template_name,
                self.config.planner_plateau_window,
                self.config.planner_plateau_threshold,
            );
            let similar = store.find_similar(
                &goal.raw,
                &graph,
                1,
                self.config.goal_similarity_weight,
                self.config.structural_similarity_weight,
                self.config.embedding_dim,
            );
            let bootstrap_seed = similar.templates.into_iter().next().map(|s| {
                let seed_graph = store.load(&s.entry.goal).ok();
                let node_summaries = seed_graph.as_ref().map(|g| {
                    g.nodes.iter()
                        .map(|n| format!("{}: {}", n.id, n.description))
                        .collect::<Vec<_>>()
                }).unwrap_or_default();
                planner_session::BootstrapSeed {
                    goal: s.entry.goal.clone(),
                    similarity_score: s.score,
                    reward: s.entry.reward,
                    node_summaries,
                    capability_set: s.entry.capability_set.clone(),
                    node_count: s.entry.node_count,
                    edge_count: s.entry.edge_count,
                }
            });
            let reward_ctx = planner_session::RewardContext {
                recent_rewards: recent,
                plateaued,
                best_reward: store.stored_reward(&template_name),
                stored_reward: store.stored_reward(&template_name),
                bootstrap_seed,
            };
            planner_session.set_reward_context(reward_ctx);
            scheduler::run_planner_execution_loop(
                &mut planner_session,
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
                &mut store,
                &template_name,
            )
            .await
        }
    }
}

fn list_workspace_entries(root: &Path, limit: usize) -> String {
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

fn ensure_unique_node_ids(nodes: &mut Vec<dag::TaskNode>) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in nodes.iter_mut() {
        let count = counts.entry(n.id.clone()).or_insert(0);
        if *count > 0 {
            let new_id = format!("{}__{}", n.id, *count);
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

    async fn run_tick(&self, ctx: &PipelineContext, _ir: &mut SystemState, _layout: &mut FileTopology) -> Result<PipelineOutcome> {
        match self.run_capability_loop(ctx).await {
            Ok(reward) => Ok(PipelineOutcome { reward, summary: "capability completed".into(), advanced: true }),
            Err(e) => Ok(PipelineOutcome { reward: -1.0, summary: format!("capability error: {e}"), advanced: false }),
        }
    }
}
