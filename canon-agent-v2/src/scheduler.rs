use super::capability::capability_model_dominant_class;
use super::capability::PipelineCapability;
use super::capability_cost::CapabilityCostCapabilityCostTable;
use super::config::{self, CapabilityConfig};
use super::console;
use super::dag;
use super::decompose;
use super::dispatch;
use super::engine;
use super::engine::TabManagerHandle;
use super::execution_result::{self, RepairAttemptStats};
use super::failure_store::{FailureStore, FailureStoreConstraint, FailureStoreConstraintRule};
use super::gpu_scheduler::driver::GpuScheduler;
use super::graph_algo::{compute_graph_features_parallel, graph_analysis_compute_graph_signals, graph_analysis_edge_count, graph_analysis_normalize_features, graph_analysis_planner_signals_for_graph, hash_graph_structure, score_node_utility, GraphFeatureVector};
use super::graph_maintenance::{self, GraphRepairMaintenanceCtx};
use super::graph_runtime::collect_execution_context;
use super::invariants;
use super::goal::GoalSpec;
use super::planner_session::{planner_controller_auto_repair_planner_update, planner_controller_validate_planner_update, PlannerController};
use super::planner_state::{PlannerStage, PlannerStagePersist, PlannerTransition, PLANNER_TRANSITIONS};
use super::planner_update::{apply_graph_patch, GraphPatch, PlannerUpdateEdgeSpec};
use super::policy;
use super::policy_engine;
use super::policy_train::{self, PolicyTrainingPolicyDatasetEntry};
use super::scheduler_scoring;
use super::scheduler_state::{ExecutionEvent, ExecutionStep, EXEC_TRANSITIONS};
use super::state_snapshot;
use super::telemetry::{self, ExecutionTelemetry, PlannerTelemetry, RuntimeTelemetry, TelemetryFrame};
use super::template_mutation;
use super::templates::GraphTemplateStore;
use super::LOG_ROOT;
use super::TEMPLATE_ROOT;
use crate::objectives;
use crate::ws_server::WsBridge;
use anyhow::Result;
use futures_util::future::join_all;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
#[derive(Clone, Copy)]
#[repr(u8)]
enum SchedulerState {
    Running,
    Blocked,
    Stop,
}
#[derive(Clone, Copy)]
#[repr(u8)]
enum SchedulerEvent {
    Completed,
    Blocked,
    Retry,
}
#[derive(Clone)]
pub struct ExecutionSchedulerExecFailure {
    pub kind: &'static str,
    pub iter: u64,
}
fn update_adaptive_concurrency(
    current: usize,
    max_cfg: usize,
    features: &GraphFeatureVector,
    exec_metrics: &ExecutionTelemetry,
    alpha: f64,
) -> usize {
    let failure_rate = features.failed_fraction.max(0.0);
    let scale = 1.0 / (1.0 + alpha * failure_rate);
    let mut next = ((current as f64) * scale).round() as usize;
    if exec_metrics.avg_latency_ms > 0 {
        // If latency is high, dampen growth.
        next = next.min(current);
    }
    next.clamp(1, max_cfg.max(1))
}

fn planner_entropy_from_history(history: &[String]) -> f64 {
    if history.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for raw in history {
        let entry = raw.trim();
        *counts.entry(entry).or_insert(0) += 1;
    }
    let total = history.len() as f64;
    let mut entropy = 0.0;
    for count in counts.values() {
        let p = *count as f64 / total;
        if p > 0.0 {
            entropy -= p * p.ln();
        }
    }
    let max_entropy = (counts.len() as f64).ln().max(1.0);
    (entropy / max_entropy).clamp(0.0, 1.0)
}
fn planner_constraints_text(constraints: &[FailureStoreConstraint]) -> String {
    if constraints.is_empty() {
        return "none\n".to_string();
    }
    let mut out = String::new();
    for c in constraints {
        let rule = match &c.rule {
            FailureStoreConstraintRule::NoCycle => "NoCycle".to_string(),
            FailureStoreConstraintRule::NoUnreachable => "NoUnreachable".to_string(),
            FailureStoreConstraintRule::CapabilityConflict => "CapabilityConflict".to_string(),
            FailureStoreConstraintRule::InvalidDependency => "InvalidDependency".to_string(),
            FailureStoreConstraintRule::PatternRewrite { pattern, replacement } => {
                format!("PatternRewrite({} -> {})", pattern, replacement)
            }
            FailureStoreConstraintRule::SignatureBan => "SignatureBan".to_string(),
        };
        out.push_str(&format!("- {} signature={}\n", rule, c.signature));
    }
    out
}
const PIPELINE_TRANSITIONS: [[SchedulerState; 3]; 3] = {
    let mut t = [[SchedulerState::Running; 3]; 3];
    t[SchedulerState::Running as usize][SchedulerEvent::Completed as usize] = SchedulerState::Stop;
    t[SchedulerState::Running as usize][SchedulerEvent::Blocked as usize] = SchedulerState::Blocked;
    t[SchedulerState::Running as usize][SchedulerEvent::Retry as usize] = SchedulerState::Running;
    t[SchedulerState::Blocked as usize][SchedulerEvent::Retry as usize] = SchedulerState::Running;
    t[SchedulerState::Blocked as usize][SchedulerEvent::Completed as usize] = SchedulerState::Stop;
    t[SchedulerState::Blocked as usize][SchedulerEvent::Blocked as usize] = SchedulerState::Blocked;
    t
};
pub(crate) async fn run_execution_loop(
    graph: &mut dag::ExecutionGraph, bridge: &WsBridge, config: &CapabilityConfig, role_rr: &tokio::sync::Mutex<HashMap<String, usize>>, tabs: &TabManagerHandle, cwd: &[PathBuf],
    workspace_listing: &str, endpoint: &config::CapabilityConfigLlmEndpoint, exec_role: &str, policy: &config::CapabilityConfigCapabilityPolicy, context_radius: usize, max_concurrency: usize,
    max_iterations: u64, tab_cooldown_ms: u64, retry_count: u32, retry_delay: u64, max_output_lines: usize, execution_preference: f64, cost_table: &mut CapabilityCostCapabilityCostTable,
    exec_metrics: &mut ExecutionTelemetry,
    goal: &GoalSpec,
) -> Result<(u64, Vec<ExecutionSchedulerExecFailure>)> {
    if graph.nodes.is_empty() {
        return Ok((0, vec![ExecutionSchedulerExecFailure { kind: "empty_graph", iter: 0 }]));
    }
    let mut adaptive_concurrency = max_concurrency.max(1);
    let mut semaphore_capacity = adaptive_concurrency;
    let semaphore = Arc::new(Semaphore::new(adaptive_concurrency));
    let mut blocked_streak = 0u32;
    let mut state = SchedulerState::Running;
    let mut failures = Vec::new();
    let mut repair_stats = RepairAttemptStats::default();
    for iter in 1..=max_iterations {
        let progress_before = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.status, dag::NodeStatus::Completed | dag::NodeStatus::Failed))
            .count();
        let completed_count = graph.nodes.iter().filter(|n| n.status == dag::NodeStatus::Completed).count();
        let failed_count = graph.nodes.iter().filter(|n| n.status == dag::NodeStatus::Failed).count();
        eprintln!(
            "[logs] execution iter={} completed={} failed={} total={}",
            iter,
            completed_count,
            failed_count,
            graph.nodes.len()
        );
        exec_metrics.last_snapshot_written = false;
        repair_stats = RepairAttemptStats::default();
        let mut ready_ids: Vec<String> = Vec::new();
        let mut features = compute_graph_features_parallel(graph);
        let mut step = ExecutionStep::CollectReady;
        let mut next_event = ExecutionEvent::Continue;
        let mut results: Vec<Result<(String, Result<engine::ModuleNodeCallResult>, std::time::Duration)>> = Vec::new();
        let mut skip_iter = false;
        loop {
            match step {
                ExecutionStep::CollectReady => {
                    let cleared = invariants::must_clear_orphan_running(graph, policy.max_node_retries);
                    if cleared > 0 {
                        eprintln!(
                            "[logs] orphan_running iter={} cleared={}",
                            iter, cleared
                        );
                    }
                    features = compute_graph_features_parallel(graph);
                    if GpuScheduler::detect_deadlock(graph) {
                        failures.push(ExecutionSchedulerExecFailure { kind: "deadlock", iter });
                        let payload = serde_json::json!(
                            { "reason" : "deadlock", "iter" : iter, }
                        );
                        let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                        let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                        cost_table.snapshot_store_save();
                        return Ok((iter, failures));
                    }
                    if ready_ids.is_empty() && !graph.all_completed() && !graph.has_failed() {
                        failures.push(ExecutionSchedulerExecFailure { kind: "no_ready", iter });
                        let payload = serde_json::json!(
                            { "reason" : "no_ready", "iter" : iter, }
                        );
                        let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                        let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                        cost_table.snapshot_store_save();
                        return Ok((iter, failures));
                    }
                    ready_ids = GpuScheduler::schedule(graph);
                    for id in &ready_ids {
                        let _ = graph.update_status(id, dag::NodeStatus::Ready);
                    }
                    let status_snapshot = graph.nodes.iter().map(|n| serde_json::json!({ "id" : n.id, "status" : n.status })).collect::<Vec<_>>();
                    if ready_ids.is_empty() && !graph.all_completed() && !graph.has_failed() {
                        if GpuScheduler::detect_deadlock(graph) {
                            failures.push(ExecutionSchedulerExecFailure { kind: "deadlock", iter });
                            let payload = serde_json::json!(
                                { "reason" : "deadlock", "iter" : iter, }
                            );
                            let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                            let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                        }
                    }
                    if graph.nodes.iter().any(|n| n.readonly_fail_count > policy.max_node_retries) {
                        failures.push(ExecutionSchedulerExecFailure { kind: "verify_loop", iter });
                    }
                    let summary = serde_json::json!(
                        { "iter" : iter, "ready" : ready_ids.clone(), "status" :
                        status_snapshot }
                    );
                    let status_path = Path::new(LOG_ROOT).join(format!("iter_{:03}_status.json", iter));
                    let _ = std::fs::write(status_path, serde_json::to_string_pretty(&summary).unwrap_or_default());
                    let completed_count = graph.nodes.iter().filter(|n| n.status == dag::NodeStatus::Completed).count();
                    let failed_count = graph.nodes.iter().filter(|n| n.status == dag::NodeStatus::Failed).count();
                    eprintln!("{}", console::console_ui_phase("tick", &format!("iter={} ready={} completed={}/{} failed={}", iter, ready_ids.len(), completed_count, graph.nodes.len(), failed_count)));
                    let event = if graph.all_completed() {
                        SchedulerEvent::Completed
                    } else if graph.has_failed() && graph.ready_nodes().is_empty() {
                        SchedulerEvent::Blocked
                    } else {
                        SchedulerEvent::Retry
                    };
                    if matches!(event, SchedulerEvent::Blocked) {
                        // I9: scheduler only blocks when no runnable nodes exist
                        invariants::must_blocked_has_no_ready(graph);
                    }
                    state = PIPELINE_TRANSITIONS[state as usize][event as usize];
                    match (state, event) {
                        (SchedulerState::Stop, _) => return Ok((iter, failures)),
                        (_, SchedulerEvent::Blocked) => {
                            blocked_streak += 1;
                            eprintln!(r#"[capability] {{\"iter\":{},\"event\":\"blocked\",\"streak\":{}}}"#, iter, blocked_streak);
                            if blocked_streak >= 3 {
                                let payload = serde_json::json!(
                                    { "reason" : "blocked", "iter" : iter, }
                                );
                                let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                                let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                                cost_table.snapshot_store_save();
                                return Ok((iter, failures));
                            }
                            next_event = ExecutionEvent::Blocked;
                            skip_iter = true;
                        }
                        _ => blocked_streak = 0,
                    }
                    if skip_iter {
                        break;
                    }
                    adaptive_concurrency = update_adaptive_concurrency(
                        adaptive_concurrency,
                        max_concurrency,
                        &features,
                        exec_metrics,
                        0.1,
                    );
                    if adaptive_concurrency > semaphore_capacity {
                        semaphore.add_permits(adaptive_concurrency - semaphore_capacity);
                        semaphore_capacity = adaptive_concurrency;
                    }
                    next_event = ExecutionEvent::Continue;
                }
                ExecutionStep::Dispatch => {
                    eprintln!(
                        "[logs] dispatch iter={} ready={}",
                        iter,
                        ready_ids.len()
                    );
                    let mut scored = scheduler_scoring::scheduler_scoring_score_ready_nodes(&ready_ids, graph, &features, cost_table, execution_preference, config);
                    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    ready_ids = scored.into_iter().map(|(id, _)| id).collect();
                    let mut futures = Vec::new();
                    for node_id in ready_ids.clone() {
                        let node = match graph.get_node(&node_id).cloned() {
                            Some(n) => n,
                            None => continue,
                        };
                        let auth = match dag::task_graph_grant_authority(&node) {
                            Ok(a) => a,
                            Err(e) => {
                                eprintln!(r#"[capability] {{\"iter\":{},\"event\":\"authority_error\",\"node\":\"{}\",\"error\":\"{}\"}}"#, iter, node.id, e);
                                let _ = graph.update_status(&node.id, dag::NodeStatus::Ready);
                                continue;
                            }
                        };
                        let sem = semaphore.clone();
                        let ctx = dispatch::node_dispatch_resolve_endpoint(
                            config,
                            role_rr,
                            exec_role,
                            (&endpoint.id, &endpoint.url, endpoint.max_tabs, endpoint.stateful, &endpoint.role_markdown),
                            cwd[0].clone(),
                            Path::new(LOG_ROOT).to_path_buf(),
                        )
                        .await;
                        let mode = capability_model_dominant_class(&node.required_capabilities);
                        let mode_label = console::console_ui_mode_tag(mode);
                        dispatch::node_dispatch_log_dispatch(&node, &mode_label, &ctx.endpoint_id);
                        let context = collect_execution_context(graph, &node.id, context_radius);
                        let fut = dispatch::dispatch_node_execution(node, auth, bridge, tabs, sem, ctx, context, iter, retry_count, retry_delay, tab_cooldown_ms);
                        futures.push(fut);
                    }
                    results = join_all(futures).await;
                }
                ExecutionStep::ApplyResults => {
                    let mut total_ms = 0u128;
                    let mut count = 0u64;
                    for item in results.drain(..) {
                        if let Some(ms) = execution_result::apply_node_result(
                            item,
                            graph,
                            cwd,
                            max_output_lines,
                            iter,
                            policy,
                            exec_metrics,
                            &mut repair_stats,
                            config.repair_radius,
                            config.max_repairs_per_node,
                            cost_table,
                            config.cost_decay_rate,
                            config.cost_latency_weight,
                            config.cost_failure_weight,
                        ) {
                            count += 1;
                            total_ms = total_ms.saturating_add(ms);
                        }
                    }
                    exec_metrics.last_repair_attempts = repair_stats.attempts;
                    exec_metrics.last_repair_successes = repair_stats.successes;
                    exec_metrics.last_repair_kind = repair_stats.last_kind.clone();
                    exec_metrics.nodes_executed = exec_metrics.nodes_executed.saturating_add(count);
                    if count > 0 {
                        let avg = (total_ms / count as u128) as u64;
                        exec_metrics.avg_latency_ms = telemetry::telemetry_update_avg_u64(exec_metrics.avg_latency_ms, avg);
                    }
                    eprintln!(
                        "[logs] apply_results iter={} applied={} avg_latency_ms={}",
                        iter,
                        count,
                        exec_metrics.avg_latency_ms
                    );
                }
                ExecutionStep::MaintainGraph => {
                    let prev_terminal_count = graph
                        .nodes
                        .iter()
                        .filter(|n| matches!(n.status, dag::NodeStatus::Completed | dag::NodeStatus::Failed))
                        .count();
                    eprintln!(
                        "[logs] graph_maintenance iter={} terminal_before={}",
                        iter,
                        prev_terminal_count
                    );
                    graph_maintenance::repair_graph(GraphRepairMaintenanceCtx {
                        graph,
                        log_dir: Path::new(LOG_ROOT),
                        iter,
                        goal: Some(goal),
                        features_retry_rate: features.retry_rate,
                        features_failed_fraction: features.failed_fraction,
                        features_branching_factor: features.branching_factor,
                        prune_unlinked: config.prune_unlinked,
                        auto_prune: config.auto_prune,
                        prune_min_age: config.prune_min_age,
                        prune_threshold: config.prune_threshold,
                        recovery_retry_rate_threshold: config.recovery_retry_rate_threshold,
                        recovery_failed_fraction_threshold: config.recovery_failed_fraction_threshold,
                    })?;
                    let next_terminal_count = graph
                        .nodes
                        .iter()
                        .filter(|n| matches!(n.status, dag::NodeStatus::Completed | dag::NodeStatus::Failed))
                        .count();
                    eprintln!(
                        "[logs] graph_maintenance iter={} terminal_after={}",
                        iter,
                        next_terminal_count
                    );
                    // I10: retry only if something changed
                    next_event = if invariants::must_retry_only_if_terminal_progress(prev_terminal_count, next_terminal_count) {
                        ExecutionEvent::Continue
                    } else {
                        ExecutionEvent::Blocked
                    };
                    // I15: progress_{t+1} >= progress_t
                    let progress_after = graph
                        .nodes
                        .iter()
                        .filter(|n| matches!(n.status, dag::NodeStatus::Completed | dag::NodeStatus::Failed))
                        .count();
                    invariants::must_progress_monotonic(progress_before, progress_after);
                    if config.enable_resume && config.snapshot_interval_iters > 0 && iter % config.snapshot_interval_iters == 0 && !exec_metrics.last_snapshot_written {
                        let snapshot = state_snapshot::PipelineSnapshot { graph: graph.clone(), iteration: iter, goal: goal.clone() };
                        state_snapshot::snapshot_store_save(Path::new(&config.snapshot_file), &snapshot);
                        exec_metrics.last_snapshot_written = true;
                        eprintln!("{}", console::console_ui_info("snapshot", &format!("wrote {}", config.snapshot_file)));
                    }
                    break;
                }
            }
            step = EXEC_TRANSITIONS[step as usize][next_event as usize];
        }
        if skip_iter {
            continue;
        }
    }
    cost_table.snapshot_store_save();
    anyhow::bail!("iteration limit exceeded")
}
pub(crate) async fn run_planner_loop(
    planner: &mut PlannerController,
    graph: &mut dag::ExecutionGraph,
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &TabManagerHandle,
    cwd: &[PathBuf],
    workspace_listing: &str,
    endpoint: &config::CapabilityConfigLlmEndpoint,
    exec_role: &str,
    policy: &config::CapabilityConfigCapabilityPolicy,
    context_radius: usize,
    max_concurrency: usize,
    max_iterations: u64,
    tab_cooldown_ms: u64,
    retry_count: u32,
    retry_delay: u64,
    max_output_lines: usize,
    store: &mut GraphTemplateStore,
    template_name: &str,
    start_stage: PlannerStage,
    planner_stage_path: Option<&Path>,
    tick: u64,
) -> Result<f64> {
    let template_hash = store.hash_for(template_name);
    let mut failure_store = FailureStore::snapshot_store_load(&template_hash);
    let mut cost_table = CapabilityCostCapabilityCostTable::snapshot_store_load();
    let mut planner_metrics = PlannerTelemetry::default();
    let mut exec_metrics = ExecutionTelemetry::default();
    let mut empty_streak = 0u32;
    let mut iter = 0u64;
    let mut last_template_reuse = false;
    let mut last_template_score = 0.0;
    let mut last_template_selected: Option<String> = None;
    let mut last_goal_similarity = 0.0;
    let mut last_template_by_embedding = false;
    let mut last_embedding_cache_hits = 0u64;
    let mut last_mutations = 0u64;
    let mut last_mutation_success = 0u64;
    let mut last_mutation_reward_delta = 0.0;
    let mut last_objective_delta = 0.0;
    let mut planner_history: Vec<String> = Vec::new();
    let mut template_hits = 0u64;
    let mut reuse_decisions = 0u64;
    let mut resume_iteration = telemetry::telemetry_resume_iteration();
    let mut prev_bias: Option<policy::PolicyModelPolicyBias> = None;
    let mut last_signal_sig = String::new();
    let mut last_completed = 0usize;
    let mut stagnant_iters = 0u64;
    let mut phase = start_stage;
    if let Some(path) = planner_stage_path {
        PlannerStagePersist::save(path, phase, tick);
    }
    while !graph.all_completed() && iter < max_iterations {
        eprintln!("{}", console::console_ui_phase("planner", &format!("iter={} nodes={}", iter, graph.nodes.len())));
        eprintln!("[planner] iter={} nodes={} stage={:?}", iter, graph.nodes.len(), phase);
        let iter_start = std::time::Instant::now();
        let completed_now = graph.nodes.iter().filter(|n| n.status == dag::NodeStatus::Completed).count();
        if completed_now <= last_completed {
            stagnant_iters = stagnant_iters.saturating_add(1);
        } else {
            stagnant_iters = 0;
        }
        last_completed = completed_now;
        let failure_stats = failure_store.stats();
        let features = compute_graph_features_parallel(graph).with_failure_stats(&failure_stats);
        let normalized = graph_analysis_normalize_features(&features, config.max_nodes, config.max_nodes.saturating_mul(4));
        let policy_outcome = policy_engine::evaluate_policy_normalized(normalized);
        let mut policy_bias = policy_outcome.bias.clone();
        let policy_decision = policy_outcome.decision.clone();
        let mut run_planner = policy_decision.run_planner;
        let mut expansion_scale = policy_decision.expansion_scale;
        let execution_preference = policy_decision.execution_preference;
        let drift = 1.0 - telemetry::telemetry_goal_similarity(graph, planner.goal_spec());
        let mut planner_refocus = false;
        if drift > config.goal_drift_threshold {
            policy_bias.planner_bias += config.goal_refocus_strength;
            policy_bias.rewrite_bias += config.goal_refocus_rewrite_strength;
            planner_refocus = true;
        }
        let current_sig = hash_graph_structure(graph);
        let signal_changed = !current_sig.is_empty() && current_sig != last_signal_sig;
        if signal_changed {
            last_signal_sig = current_sig;
            run_planner = true;
        }
        if stagnant_iters >= 2 {
            run_planner = true;
        }
        if graph.nodes.is_empty() || features.deadlock_rate > 0.0 {
            run_planner = true;
        }
        if expansion_scale.is_nan() || expansion_scale <= 0.0 {
            expansion_scale = 1.0;
        }
        let rewrite_requests = graph.nodes.iter().filter_map(|n| n.reasoning_trace.as_ref().and_then(|trace| trace.starts_with("REWRITE_REQUESTED").then(|| n.id.clone()))).collect::<Vec<_>>();
        let mut reuse_decision = false;
        let mut reuse_score = 0.0;
        let mut reuse_goal: Option<String> = None;
        let mut reuse_goal_similarity = 0.0;
        let mut reuse_by_embedding = false;
        last_embedding_cache_hits = 0;
        if matches!(phase, PlannerStage::ReuseTemplate) {
            if !run_planner {
                let search = store.find_similar(
                    planner.goal_spec(),
                    graph,
                    config.template_top_k,
                    config.goal_similarity_weight,
                    config.structural_similarity_weight,
                    config.template_failure_hard_ban,
                );
                last_embedding_cache_hits = search.cache_hits;
                if let Some(best) = search.templates.into_iter().max_by(|a, b| {
                    let a_score = a.score * a.entry.reward;
                    let b_score = b.score * b.entry.reward;
                    a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    reuse_score = best.score * best.entry.reward;
                    reuse_goal = Some(best.entry.goal.clone());
                    reuse_goal_similarity = best.goal_similarity;
                    reuse_by_embedding = best.used_embedding;
                    if reuse_score >= config.template_reuse_threshold {
                        if let Ok(loaded) = store.snapshot_store_load(&best.entry.goal) {
                            *graph = loaded;
                            graph.reset_for_execution();
                            graph.rebuild_index();
                            reuse_decision = true;
                            template_hits += 1;
                        }
                    }
                }
            }
            last_template_reuse = reuse_decision;
            last_template_score = reuse_score;
            last_template_selected = reuse_goal.clone();
            last_goal_similarity = reuse_goal_similarity;
            last_template_by_embedding = reuse_by_embedding && reuse_decision;
            reuse_decisions += 1;
            if !reuse_decision && !run_planner {
                run_planner = true;
            }
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerTransition::ReuseDone as usize];
            if let Some(path) = planner_stage_path {
                PlannerStagePersist::save(path, phase, tick);
            }
        }
        let recovery_reason = engine::module_take_recovery_signal(Path::new(LOG_ROOT));
        let mut rewrite_requests = rewrite_requests;
        if let Some(reason) = recovery_reason.as_ref() {
            for node in &graph.nodes {
                if matches!(node.status, dag::NodeStatus::Pending | dag::NodeStatus::Failed) {
                    rewrite_requests.push(node.id.clone());
                }
            }
            eprintln!("{}", console::console_ui_info("recovery", &format!("reason={} rewrites={}", reason, rewrite_requests.len())));
        }
        let signals = graph_analysis_compute_graph_signals(graph);
        let log_dir = Path::new(LOG_ROOT).join("planner_logs");
        let mut revision_rewrites = None;
        let mut last_update_counts = None;
        let mut update = None;
        let mut constraint_rejections = 0u64;
        let mut constraint_types: Vec<String> = Vec::new();
        let attempts = retry_count.max(1);
        let force_planner_expand = store.is_plateaued(template_name, config.planner_plateau_window, config.planner_plateau_threshold) || features.ready_fraction < 0.1 || recovery_reason.is_some();
        let remaining_nodes = config.max_nodes.saturating_sub(graph.nodes.len());
        let (mut planner_max_new_nodes, mut planner_max_new_edges) = if force_planner_expand {
            (
                (config.planner_max_new_nodes.saturating_mul(config.planner_plateau_expand_factor)).min(remaining_nodes.max(1)),
                config.planner_max_new_edges.saturating_mul(config.planner_plateau_expand_factor),
            )
        } else {
            (config.planner_max_new_nodes, config.planner_max_new_edges)
        };
        let run_planner_now = run_planner && !reuse_decision;
        if run_planner_now {
            planner_max_new_nodes = ((planner_max_new_nodes as f64) * expansion_scale).round().max(1.0).min(remaining_nodes.max(1) as f64) as usize;
            planner_max_new_edges = ((planner_max_new_edges as f64) * expansion_scale).round().max(1.0) as usize;
        }
        if matches!(phase, PlannerStage::MutateTemplate) && force_planner_expand && config.mutation_candidates > 0 {
            let base_reward = store.stored_reward(template_name);
            let target_symbols = planner.goal_spec().artifact.as_ref().map(|a| a.target_symbols.clone()).unwrap_or_default();
            let mut base_graph = graph.clone();
            let seed = store.find_similar(
                planner.goal_spec(),
                graph,
                1,
                config.goal_similarity_weight,
                config.structural_similarity_weight,
                config.template_failure_hard_ban,
            );
            if let Some(best) = seed.templates.into_iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)) {
                if let Ok(mut loaded) = store.snapshot_store_load(&best.entry.goal) {
                    loaded.reset_for_execution();
                    loaded.rebuild_index();
                    base_graph = loaded;
                }
            }
            let candidates = template_mutation::generate_mutation_candidates(
                &base_graph,
                config.mutation_candidates,
                config.mutation_budget,
                config.mutation_rate,
                iter,
                &target_symbols,
            );
            let mut scored = template_mutation::score_mutation_candidates(candidates, iter);
            scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            last_mutations = scored.len() as u64;
            let mut best_reward = base_reward;
            let mut best_graph = None;
            let mut success = 0u64;
            for candidate in scored {
                if candidate.graph.validate().is_err() {
                    continue;
                }
                let mut eval_graph = candidate.graph.clone();
                if let Ok((_, _)) = run_execution_loop(
                    &mut eval_graph,
                    bridge,
                    config,
                    role_rr,
                    tabs,
                    cwd,
                    workspace_listing,
                    endpoint,
                    exec_role,
                    policy,
                    context_radius,
                    max_concurrency,
                    config.max_expand_iters as u64,
                    tab_cooldown_ms,
                    retry_count,
                    retry_delay,
                    max_output_lines,
                    execution_preference,
                    &mut cost_table,
                    &mut exec_metrics,
                    planner.goal_spec(),
                )
                .await
                {
                    let reward = telemetry::telemetry_compute_reward(&eval_graph, 1, config.max_expand_iters as u64, planner.goal_spec());
                    success += 1;
                    if reward > best_reward {
                        best_reward = reward;
                        best_graph = Some(eval_graph);
                    }
                }
            }
            last_mutation_success = success;
            last_mutation_reward_delta = best_reward - base_reward;
            if let Some(best_graph) = best_graph {
                *graph = best_graph;
                let _ = store.save_with_reward(template_name, graph, best_reward);
            }
        }
        if matches!(phase, PlannerStage::MutateTemplate) {
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerTransition::MutationDone as usize];
            if let Some(path) = planner_stage_path {
                PlannerStagePersist::save(path, phase, tick);
            }
        }
        if matches!(phase, PlannerStage::GraphPatch) && run_planner_now {
            for attempt in 1..=attempts {
                planner_metrics.planner_calls += 1;
                if attempt > 1 {
                    planner_metrics.planner_retries += 1;
                }
                let mut features_for_bias = features.clone();
                if let Some(ctx) = planner.reward_context() {
                    features_for_bias = features_for_bias.with_reward_history(&ctx.recent_rewards);
                }
                let normalized = graph_analysis_normalize_features(&features_for_bias, config.max_nodes, config.max_nodes.saturating_mul(4));
                let bias_raw = policy::ExecutionPolicyModel::load_default().predict(&normalized);
                let bias_smoothed = policy::policy_model_smooth_bias(prev_bias.as_ref(), bias_raw);
                let bias = policy::policy_model_maybe_explore(bias_smoothed, 0.05);
                prev_bias = Some(bias.clone());
                let bias_text = policy::policy_model_format_bias(&bias);
                let constraints = failure_store.constraints(config.failure_constraint_threshold, config.max_constraints);
                let constraints_text = planner_constraints_text(&constraints);
                let graph_signals_text = graph_analysis_planner_signals_for_graph(graph);
                let feature_vector_text = serde_json::to_string_pretty(&features).unwrap_or_default();
                let refocus_text = if planner_refocus {
                    format!(
                        "GOAL_REFOCUS: enabled=true strength={:.3}\n",
                        config.goal_refocus_rewrite_strength
                    )
                } else {
                    String::new()
                };
                let prompt = planner.build_prompt(
                    graph,
                    &signals,
                    &features,
                    &cost_table.summary(5, config.cost_latency_weight, config.cost_failure_weight),
                    &rewrite_requests,
                    &bias_text,
                    planner_max_new_nodes,
                    planner_max_new_edges,
                    &constraints_text,
                    &graph_signals_text,
                    &feature_vector_text,
                    &refocus_text,
                );
                let mut candidate = GraphPatch { new_nodes: Vec::new(), new_edges: Vec::new(), retract_nodes: Vec::new(), rewrite_nodes: Vec::new() };
                let attempts = retry_count.max(1);
                for attempt in 1..=attempts {
                    let allow_mismatch = attempt > 1 && planner.is_history_empty();
                    let raw = engine::module_call_llm_raw_with_retry_allow_mismatch(
                        bridge,
                        &endpoint.id,
                        &endpoint.url,
                        endpoint.stateful,
                        &prompt,
                        &endpoint.role_markdown,
                        "planner",
                        None,
                        tabs,
                        endpoint.max_tabs,
                        tab_cooldown_ms,
                        retry_count,
                        retry_delay,
                    )
                    .await;
                    let raw = match raw {
                        Ok(v) => v,
                        Err(e) => {
                            if e.to_string().contains("req_id mismatch") {
                                planner.clear_history();
                                let retry_raw = engine::module_call_llm_raw_with_retry_allow_mismatch(
                                    bridge,
                                    &endpoint.id,
                                    &endpoint.url,
                                    endpoint.stateful,
                                    &prompt,
                                    &endpoint.role_markdown,
                                    "planner",
                                    None,
                                    tabs,
                                    endpoint.max_tabs,
                                    tab_cooldown_ms,
                                    retry_count,
                                    retry_delay,
                                )
                                .await?;
                                retry_raw
                            } else if attempt < attempts {
                                continue;
                            } else {
                                return Err(e);
                            }
                        }
                    };
                    match planner.apply_raw_response(raw.clone(), &log_dir, iter, graph.nodes.len(), &signals) {
                        Ok(update) => {
                            planner_history.push(raw);
                            candidate = update;
                            break;
                        }
                        Err(e) => {
                            if attempt < attempts {
                                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                                continue;
                            }
                            return Err(e);
                        }
                    }
                }
                let repaired = planner_controller_auto_repair_planner_update(graph, &mut candidate);
                if repaired.count > 0 {
                    let ids = repaired.ids.join(", ");
                    eprintln!("[planner] auto-repaired {} mixed-class nodes: [{}]", repaired.count, ids);
                }
                let output_payload = serde_json::json!(
                    { "iter" : iter, "attempt" : attempt, "auto_repaired" : repaired
                    .count, "auto_repair_ids" : repaired.ids, "planner_output" :
                    candidate, }
                );
                let output_path = log_dir.join(format!("planner_iter_{:04}_output.json", iter));
                if let Ok(pretty) = serde_json::to_string_pretty(&output_payload) {
                    let _ = std::fs::write(output_path, pretty);
                }
                let mut candidate_graph = graph.clone();
                if let Err(e) = apply_graph_patch(&mut candidate_graph, candidate.clone()) {
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                    planner_metrics.planner_failures += 1;
                    return Err(e);
                }
                let candidate_sig = hash_graph_structure(&candidate_graph);
                if failure_store.contains(&candidate_sig) {
                    let payload = serde_json::json!(
                        { "iter" : iter, "attempt" : attempt, "error" :
                        "planner candidate matches known failure signature", "signature"
                        : candidate_sig, }
                    );
                    let path = log_dir.join(format!("planner_iter_{:04}_rejected_failure.json", iter));
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        let _ = std::fs::write(path, pretty);
                    }
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                }
                if let Err(e) = planner_controller_validate_planner_update(
                    graph,
                    &candidate,
                    planner_max_new_nodes,
                    planner_max_new_edges,
                    &mut failure_store,
                    iter,
                    config.failure_constraint_threshold,
                    config.max_constraints,
                ) {
                    let err_msg = e.to_string();
                    if err_msg.starts_with("constraint violated:") {
                        constraint_rejections += 1;
                        constraint_types.push(err_msg.replace("constraint violated: ", ""));
                    }
                    if err_msg.contains("cycle detected") || err_msg.contains("capability class") {
                        store.record_failure(&template_hash);
                    }
                    let payload = serde_json::json!(
                        { "iter" : iter, "attempt" : attempt, "error" : err_msg,
                        "planner_output" : candidate, }
                    );
                    let path = log_dir.join(format!("planner_iter_{:04}_validate_error.json", iter));
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        let _ = std::fs::write(path, pretty);
                    }
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                    planner_metrics.planner_failures += 1;
                    return Err(e);
                }
                if force_planner_expand && candidate.new_nodes.is_empty() && candidate.new_edges.is_empty() {
                    let payload = serde_json::json!(
                        { "iter" : iter, "attempt" : attempt, "error" :
                        "plateaued template requires expansion", "planner_output" :
                        candidate, }
                    );
                    let path = log_dir.join(format!("planner_iter_{:04}_expand_required.json", iter));
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        let _ = std::fs::write(path, pretty);
                    }
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                }
                update = Some(candidate);
                break;
            }
        }
        if let Some(update) = update {
            revision_rewrites = Some(update.rewrite_nodes.len());
            last_update_counts = Some((update.new_nodes.len(), update.new_edges.len(), update.rewrite_nodes.len()));
            if update.new_nodes.is_empty() && update.new_edges.is_empty() {
                empty_streak += 1;
            } else {
                empty_streak = 0;
            }
            if empty_streak >= 3 {
                break;
            }
            planner_metrics.nodes_added += update.new_nodes.len() as u64;
            planner_metrics.edges_added += update.new_edges.len() as u64;
            let mut updated = graph.clone();
            apply_graph_patch(&mut updated, update)?;
            updated.validate().map_err(|e| anyhow::anyhow!(e))?;
            store.snapshot_store_save(template_name, &updated)?;
            *graph = updated;
            eprintln!("[planner] applied update: nodes={} edges={}", graph.nodes.len(), graph_analysis_edge_count(graph));
            if let Some(rewrites) = revision_rewrites {
                if rewrites > 0 {
                    for node in &mut graph.nodes {
                        if rewrite_requests.contains(&node.id) {
                            node.reasoning_trace = None;
                        }
                    }
                }
            }
        } else {
            last_update_counts = Some((0, 0, 0));
        }
        if matches!(phase, PlannerStage::GraphPatch) {
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerTransition::PlannerDone as usize];
            if let Some(path) = planner_stage_path {
                PlannerStagePersist::save(path, phase, tick);
            }
        }
        let (exec_iters, exec_failures) = if matches!(phase, PlannerStage::Execute) {
            eprintln!("[planner] execute_start iter={} nodes={}", iter, graph.nodes.len());
            let completed_before = graph
                .nodes
                .iter()
                .filter(|n| n.status == dag::NodeStatus::Completed)
                .count();
            let res = run_execution_loop(
                graph,
                bridge,
                config,
                role_rr,
                tabs,
                cwd,
                workspace_listing,
                endpoint,
                exec_role,
                policy,
                context_radius,
                max_concurrency,
                max_iterations,
                tab_cooldown_ms,
                retry_count,
                retry_delay,
                max_output_lines,
                execution_preference,
                &mut cost_table,
                &mut exec_metrics,
                planner.goal_spec(),
            )
            .await?;
            let completed_after = graph
                .nodes
                .iter()
                .filter(|n| n.status == dag::NodeStatus::Completed)
                .count();
            if completed_after == completed_before {
                phase = PlannerStage::ReuseTemplate;
            } else {
                phase = PLANNER_TRANSITIONS[phase as usize][PlannerTransition::ExecuteDone as usize];
            }
            eprintln!(
                "[planner] execute_end iter={} completed_before={} completed_after={} next_stage={:?}",
                iter,
                completed_before,
                completed_after,
                phase
            );
            if let Some(path) = planner_stage_path {
                PlannerStagePersist::save(path, phase, tick);
            }
            res
        } else {
            (0, Vec::new())
        };
        // I14: if graph is fully stalled, replan
        let all_stalled = invariants::must_replan_if_all_stalled(graph, planner_stage_path, tick);
        if all_stalled {
            phase = PlannerStage::ReuseTemplate;
        }
        if matches!(phase, PlannerStage::Evaluate) {
            phase = PlannerStage::ReuseTemplate;
            if let Some(path) = planner_stage_path {
                PlannerStagePersist::save(path, phase, tick);
            }
        }
        for failure in exec_failures {
            failure_store.record_graph(failure.kind, graph, failure.iter);
            store.record_failure_and_maybe_evict(template_name, config.template_population_size);
        }
        let _exec_iters = exec_iters;
        let iterations_used = iter.saturating_add(1);
        planner_metrics.iterations += 1;
        let reward = telemetry::telemetry_compute_reward(graph, iterations_used, max_iterations, planner.goal_spec());
        let policy_prediction = policy_bias.planner_bias;
        let policy_error = reward - policy_prediction;
        let avg_node_utility = if graph.nodes.is_empty() {
            0.0
        } else {
            let mut total = 0.0;
            for n in &graph.nodes {
                let cost = cost_table.node_cost(&n.required_capabilities, config.cost_latency_weight, config.cost_failure_weight);
                total += n.priority as f64 - cost;
            }
            total / graph.nodes.len() as f64
        };
        let goal_sim = telemetry::telemetry_goal_similarity(graph, planner.goal_spec());
        let mut runtime = RuntimeTelemetry::default();
        runtime.queue.queue_depth = telemetry::telemetry_pending_requests();
        runtime.queue.retry_rate = if planner_metrics.planner_calls == 0 { 0.0 } else { planner_metrics.planner_retries as f64 / planner_metrics.planner_calls as f64 };
        runtime.queue.progress_fraction = telemetry::telemetry_progress_fraction(graph);
        runtime.queue.iteration_time_ms = iter_start.elapsed().as_millis() as u64;
        runtime.queue.branching_factor = features.branching_factor;
        runtime.queue.blocked_fraction = features.blocked_fraction;
        runtime.queue.completion_velocity = features.completion_velocity;
        runtime.queue.deadlock_rate = features.deadlock_rate;
        runtime.policy.policy_prediction = policy_prediction;
        runtime.policy.policy_error = policy_error;
        runtime.policy.policy_weight_norm = policy_outcome.weight_norm;
        runtime.policy.dataset_size = policy_train::policy_training_dataset_size();
        runtime.policy.policy_run_planner = run_planner_now;
        runtime.policy.policy_expansion_scale = expansion_scale;
        runtime.policy.policy_execution_preference = execution_preference;
        runtime.template.template_reuse = last_template_reuse;
        runtime.template.template_score = last_template_score;
        runtime.template.template_selected = last_template_selected.clone();
        runtime.template.template_mutations = last_mutations;
        runtime.template.mutation_success_rate = if last_mutations == 0 { 0.0 } else { last_mutation_success as f64 / last_mutations as f64 };
        runtime.template.mutation_reward_delta = last_mutation_reward_delta;
        runtime.template.template_reuse_by_embedding = last_template_by_embedding;
        runtime.template.embedding_cache_hits = last_embedding_cache_hits;
        let current_objective_delta = objectives::objective_reward_delta();
        if current_objective_delta > last_objective_delta {
            last_objective_delta = current_objective_delta;
        }
        runtime.template.objective_delta = last_objective_delta;
        runtime.template.template_hit_rate = if reuse_decisions == 0 { 0.0 } else { template_hits as f64 / reuse_decisions as f64 };
        runtime.repair.repair_attempts = exec_metrics.last_repair_attempts;
        runtime.repair.repair_success_rate = if exec_metrics.last_repair_attempts == 0 { 0.0 } else { exec_metrics.last_repair_successes as f64 / exec_metrics.last_repair_attempts as f64 };
        runtime.repair.repair_type = exec_metrics.last_repair_kind.clone();
        runtime.repair.constraint_rejections = constraint_rejections;
        runtime.repair.constraint_hit_rate = if attempts == 0 { 0.0 } else { constraint_rejections as f64 / attempts as f64 };
        runtime.repair.constraint_types = if constraint_types.is_empty() { None } else { Some(constraint_types.join(",")) };
        runtime.repair.planner_entropy = planner_entropy_from_history(&planner_history);
        runtime.performance.avg_capability_latency = cost_table.avg_latency();
        runtime.performance.avg_capability_failure = cost_table.avg_failure();
        runtime.performance.avg_node_utility = avg_node_utility;
        runtime.snapshot.snapshot_written = exec_metrics.last_snapshot_written;
        runtime.snapshot.snapshot_loaded = resume_iteration > 0;
        runtime.snapshot.resume_iteration = resume_iteration;
        runtime.goal.goal_similarity_score = goal_sim;
        runtime.goal.goal_drift = (1.0 - goal_sim).clamp(0.0, 1.0);
        runtime.goal.planner_refocus = planner_refocus;
        let reward_history = store.recent_rewards(template_name, 6);
        let features = features.with_reward_history(&reward_history);
        let failures = failure_store.failure_count();
        let (add_nodes, add_edges, rewrites) = last_update_counts.unwrap_or((0, 0, 0));
        let entry = PolicyTrainingPolicyDatasetEntry {
            features: serde_json::json!(
                { "nodes" : features.nodes, "edges" : features.edges, "depth" : features
                .depth, "scc_count" : features.scc_count, "failure_rate" : features
                .failure_rate, "reward_trend" : features.reward_trend, "avg_out_degree" :
                features.avg_out_degree, "avg_in_degree" : features.avg_in_degree,
                "branching_factor" : features.branching_factor, "leaf_count" : features
                .leaf_count, "root_count" : features.root_count, "verify_to_mutate_ratio"
                : features.verify_to_mutate_ratio, "observe_to_mutate_ratio" : features
                .observe_to_mutate_ratio, "node_type_entropy" : features
                .node_type_entropy, "avg_node_priority" : features.avg_node_priority,
                "avg_node_budget" : features.avg_node_budget, "blocked_fraction" :
                features.blocked_fraction, "ready_fraction" : features.ready_fraction,
                "failed_fraction" : features.failed_fraction, "completion_velocity" :
                features.completion_velocity, "retry_rate" : features.retry_rate,
                "failure_pattern_rate" : features.failure_pattern_rate, "cycle_frequency"
                : features.cycle_frequency, "deadlock_rate" : features.deadlock_rate,
                "failures" : failures }
            ),
            action: serde_json::json!(
                { "add_nodes" : add_nodes, "add_edges" : add_edges, "rewrites" : rewrites
                }
            ),
            policy_decision: serde_json::json!(
                { "run_planner" : run_planner, "expansion_scale" : expansion_scale,
                "execution_preference" : execution_preference }
            ),
            reward,
        };
        policy_train::policy_training_append_policy_dataset(&entry);
        policy_train::policy_training_update_online(&entry, config.max_nodes, config.max_nodes.saturating_mul(4));
        if let Some(rewrites) = revision_rewrites {
            store.record_revision(template_name, graph, reward, rewrites, iter);
        }
        let snapshot =
            TelemetryFrame { planner: planner_metrics.clone(), exec: exec_metrics.clone(), runtime, reward, template_hash: Some(store.hash_for(template_name)), goal: Some(template_name.to_string()) };
        telemetry::telemetry_record_all_snapshots(&snapshot, LOG_ROOT, TEMPLATE_ROOT, &template_hash);
        iter += 1;
        resume_iteration = iter;
    }
    let reward = telemetry::telemetry_compute_reward(graph, iter, max_iterations, planner.goal_spec());
        let final_features = compute_graph_features_parallel(graph).with_failure_stats(&failure_store.stats());
    let final_entry = PolicyTrainingPolicyDatasetEntry {
        features: serde_json::json!(
            { "nodes" : final_features.nodes, "edges" : final_features.edges,
            "depth" : final_features.depth, "scc_count" : final_features.scc_count,
            "failure_rate" : final_features.failure_rate, "reward_trend" :
            final_features.reward_trend, "avg_out_degree" :
            final_features.avg_out_degree, "avg_in_degree" :
            final_features.avg_in_degree, "branching_factor" :
            final_features.branching_factor, "leaf_count" :
            final_features.leaf_count, "root_count" : final_features.root_count,
            "verify_to_mutate_ratio" : final_features.verify_to_mutate_ratio,
            "observe_to_mutate_ratio" : final_features.observe_to_mutate_ratio,
            "node_type_entropy" : final_features.node_type_entropy,
            "avg_node_priority" : final_features.avg_node_priority,
            "avg_node_budget" : final_features.avg_node_budget,
            "blocked_fraction" : final_features.blocked_fraction,
            "ready_fraction" : final_features.ready_fraction,
            "failed_fraction" : final_features.failed_fraction,
            "completion_velocity" : final_features.completion_velocity,
            "retry_rate" : final_features.retry_rate,
            "failure_pattern_rate" : final_features.failure_pattern_rate,
            "cycle_frequency" : final_features.cycle_frequency,
            "deadlock_rate" : final_features.deadlock_rate,
            "failures" : failure_store.failure_count() }
        ),
        action: serde_json::json!({ "add_nodes" : 0, "add_edges" : 0, "rewrites" : 0 }),
        policy_decision: serde_json::json!(
            { "run_planner" : false, "expansion_scale" : 1.0,
            "execution_preference" : 0.0 }
        ),
        reward,
    };
    policy_train::policy_training_append_policy_dataset(&final_entry);
    policy_train::policy_training_update_online(&final_entry, config.max_nodes, config.max_nodes.saturating_mul(4));
    if graph.all_completed() && !graph.has_failed() {
        if let Err(e) = store.save_with_reward(template_name, graph, reward) {
            eprintln!("[templates] failed to persist updated template: {}", e);
        }
    }
    store.record_reward(template_name, reward);
    Ok(reward)
}
