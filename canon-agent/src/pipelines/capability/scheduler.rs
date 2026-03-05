use crate::ws_server::WsBridge;
use anyhow::Result;
use futures_util::future::join_all;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::capability::dominant_class;
use super::capability::Capability;
use super::capability_cost::CapabilityCostTable;
use super::config::{self, CapabilityConfig};
use super::console;
use super::dag;
use super::decompose;
use super::dispatch;
use super::engine;
use super::engine::TabsHandle;
use super::execution_result::{self, RepairStats};
use super::failure_store::FailureStore;
use super::gpu_scheduler::driver::GpuScheduler;
use super::graph_algo::{compute_graph_signals, edge_count, graph_features, graph_signature, node_utility, normalize_features};
use super::graph_maintenance::{self, MaintenanceCtx};
use super::graph_runtime::build_context;
use super::planner_session::{auto_repair_planner_update, validate_planner_update, PlannerSession};
use super::planner_state::{PlannerEvent, PlannerPhase, PLANNER_TRANSITIONS};
use super::planner_update::{apply_planner_update, EdgeSpec, PlannerUpdate};
use super::policy;
use super::policy_engine;
use super::policy_train::{self, PolicyDatasetEntry};
use super::scheduler_scoring;
use super::scheduler_state::{ExecEvent, ExecStep, EXEC_TRANSITIONS};
use super::state_snapshot;
use super::telemetry::{self, ExecMetrics, PlannerMetrics, RuntimeMetrics, TelemetrySnapshot};
use super::template_mutation;
use super::templates::TemplateStore;
use super::LOG_ROOT;
use super::TEMPLATE_ROOT;

#[derive(Clone, Copy)]
#[repr(u8)]
enum PipelineState {
    Running,
    Blocked,
    Stop,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum PipelineEvent {
    Completed,
    Blocked,
    Retry,
}

#[derive(Clone)]
pub struct ExecFailure {
    pub kind: &'static str,
    pub iter: u64,
}

const PIPELINE_TRANSITIONS: [[PipelineState; 3]; 3] = {
    let mut t = [[PipelineState::Running; 3]; 3];
    t[PipelineState::Running as usize][PipelineEvent::Completed as usize] = PipelineState::Stop;
    t[PipelineState::Running as usize][PipelineEvent::Blocked as usize] = PipelineState::Blocked;
    t[PipelineState::Running as usize][PipelineEvent::Retry as usize] = PipelineState::Running;
    t[PipelineState::Blocked as usize][PipelineEvent::Retry as usize] = PipelineState::Running;
    t[PipelineState::Blocked as usize][PipelineEvent::Completed as usize] = PipelineState::Stop;
    t[PipelineState::Blocked as usize][PipelineEvent::Blocked as usize] = PipelineState::Blocked;
    t
};

pub(crate) async fn execute_graph_loop(
    graph: &mut dag::TaskGraph, bridge: &WsBridge, config: &CapabilityConfig, role_rr: &tokio::sync::Mutex<HashMap<String, usize>>, tabs: &TabsHandle, cwd: &[PathBuf], workspace_listing: &str,
    endpoint: &config::LlmEndpoint, exec_role: &str, policy: &config::CapabilityPolicy, context_radius: usize, max_concurrency: usize, max_iterations: u64, tab_cooldown_ms: u64, retry_count: u32,
    retry_delay: u64, max_output_lines: usize, execution_preference: f64, cost_table: &mut CapabilityCostTable, exec_metrics: &mut ExecMetrics,
) -> Result<(u64, Vec<ExecFailure>)> {
    let semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
    let mut blocked_streak = 0u32;
    let mut state = PipelineState::Running;
    let mut failures = Vec::new();
    let mut repair_stats = RepairStats::default();
    for iter in 1..=max_iterations {
        exec_metrics.last_snapshot_written = false;
        repair_stats = RepairStats::default();
        let mut ready_ids: Vec<String> = Vec::new();
        let mut features = graph_features(graph);
        let mut step = ExecStep::CollectReady;
        let mut next_event = ExecEvent::Continue;
        let mut results: Vec<Result<(String, Result<engine::NodeCallResult>, std::time::Duration)>> = Vec::new();
        let mut skip_iter = false;

        loop {
            match step {
                ExecStep::CollectReady => {
                    features = graph_features(graph);
                    ready_ids = GpuScheduler::schedule(graph);
                    for id in &ready_ids {
                        let _ = graph.update_status(id, dag::Status::Ready);
                    }
                    let status_snapshot = graph.nodes.iter().map(|n| serde_json::json!({"id": n.id, "status": n.status})).collect::<Vec<_>>();
                    if ready_ids.is_empty() && !graph.all_completed() && !graph.has_failed() {
                        if GpuScheduler::detect_deadlock(graph) {
                            failures.push(ExecFailure { kind: "deadlock", iter });
                            let payload = serde_json::json!({
                                "reason": "deadlock",
                                "iter": iter,
                            });
                            let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                            let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                        }
                    }
                    if graph.nodes.iter().any(|n| n.readonly_fail_count > policy.max_node_retries) {
                        failures.push(ExecFailure { kind: "verify_loop", iter });
                    }
                    let summary = serde_json::json!({
                        "iter": iter,
                        "ready": ready_ids.clone(),
                        "status": status_snapshot
                    });
                    let status_path = Path::new(LOG_ROOT).join(format!("iter_{:03}_status.json", iter));
                    let _ = std::fs::write(status_path, serde_json::to_string_pretty(&summary).unwrap_or_default());
                    eprintln!("{}", console::info("capability", &summary.to_string()));
                    let completed_count = graph.nodes.iter().filter(|n| n.status == dag::Status::Completed).count();
                    let failed_count = graph.nodes.iter().filter(|n| n.status == dag::Status::Failed).count();
                    eprintln!("{}", console::phase("tick", &format!("iter={} ready={} completed={}/{} failed={}", iter, ready_ids.len(), completed_count, graph.nodes.len(), failed_count)));

                    let event = if graph.all_completed() {
                        PipelineEvent::Completed
                    } else if graph.has_failed() && graph.ready_nodes().is_empty() {
                        PipelineEvent::Blocked
                    } else {
                        PipelineEvent::Retry
                    };
                    state = PIPELINE_TRANSITIONS[state as usize][event as usize];
                    match (state, event) {
                        (PipelineState::Stop, _) => return Ok((iter, failures)),
                        (_, PipelineEvent::Blocked) => {
                            blocked_streak += 1;
                            eprintln!(r#"[capability] {{\"iter\":{},\"event\":\"blocked\",\"streak\":{}}}"#, iter, blocked_streak);
                            if blocked_streak >= 3 {
                                let payload = serde_json::json!({
                                    "reason": "blocked",
                                    "iter": iter,
                                });
                                let path = Path::new(LOG_ROOT).join("recovery_signal.json");
                                let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
                                cost_table.save();
                                return Ok((iter, failures));
                            }
                            next_event = ExecEvent::Blocked;
                            skip_iter = true;
                        }
                        _ => blocked_streak = 0,
                    }
                    if skip_iter {
                        break;
                    }
                    next_event = ExecEvent::Continue;
                }
                ExecStep::Dispatch => {
                    let mut scored = scheduler_scoring::score_ready_nodes(&ready_ids, graph, &features, cost_table, execution_preference, config);
                    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    ready_ids = scored.into_iter().map(|(id, _)| id).collect();

                    let mut futures = Vec::new();
                    for node_id in ready_ids.clone() {
                        let node = match graph.get_node(&node_id).cloned() {
                            Some(n) => n,
                            None => continue,
                        };
                        let auth = match dag::grant_authority(&node) {
                            Ok(a) => a,
                            Err(e) => {
                                eprintln!(r#"[capability] {{\"iter\":{},\"event\":\"authority_error\",\"node\":\"{}\",\"error\":\"{}\"}}"#, iter, node.id, e);
                                let _ = graph.update_status(&node.id, dag::Status::Ready);
                                continue;
                            }
                        };
                        let sem = semaphore.clone();
                        let ctx = dispatch::resolve_endpoint(
                            config,
                            role_rr,
                            exec_role,
                            (&endpoint.id, &endpoint.url, endpoint.max_tabs, endpoint.stateful),
                            cwd[0].clone(),
                            Path::new(LOG_ROOT).to_path_buf(),
                        )
                        .await;
                        let mode = dominant_class(&node.required_capabilities);
                        let mode_label = console::mode_tag(mode);
                        dispatch::log_dispatch(&node, &mode_label, &ctx.endpoint_id);
                        let context = build_context(graph, &node.id, context_radius);
                        let fut = dispatch::dispatch_node_call(node, auth, bridge, tabs, sem, ctx, context, iter, retry_count, retry_delay, tab_cooldown_ms);
                        futures.push(fut);
                    }
                    results = join_all(futures).await;
                }
                ExecStep::ApplyResults => {
                    let mut total_ms = 0u128;
                    let mut count = 0u64;
                    for item in results.drain(..) {
                        if let Some(ms) = execution_result::process_node_result(
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
                        exec_metrics.avg_latency_ms = telemetry::update_avg_u64(exec_metrics.avg_latency_ms, avg);
                    }
                }
                ExecStep::MaintainGraph => {
                    graph_maintenance::maintain_graph(MaintenanceCtx {
                        graph,
                        log_dir: Path::new(LOG_ROOT),
                        iter,
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
                    if config.enable_resume && config.snapshot_interval_iters > 0 && iter % config.snapshot_interval_iters == 0 {
                        let snapshot = state_snapshot::StateSnapshot { graph: graph.clone(), iteration: iter };
                        state_snapshot::save(Path::new(&config.snapshot_file), &snapshot);
                        exec_metrics.last_snapshot_written = true;
                        eprintln!("{}", console::info("snapshot", &format!("wrote {}", config.snapshot_file)));
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
    cost_table.save();
    anyhow::bail!("iteration limit exceeded")
}

pub(crate) async fn run_planner_execution_loop(
    planner: &mut PlannerSession, graph: &mut dag::TaskGraph, bridge: &WsBridge, config: &CapabilityConfig, role_rr: &tokio::sync::Mutex<HashMap<String, usize>>, tabs: &TabsHandle, cwd: &[PathBuf],
    workspace_listing: &str, endpoint: &config::LlmEndpoint, exec_role: &str, policy: &config::CapabilityPolicy, context_radius: usize, max_concurrency: usize, max_iterations: u64,
    tab_cooldown_ms: u64, retry_count: u32, retry_delay: u64, max_output_lines: usize, store: &mut TemplateStore, template_name: &str,
) -> Result<f64> {
    let template_hash = store.hash_for(template_name);
    let mut failure_store = FailureStore::load(&template_hash);
    let mut cost_table = CapabilityCostTable::load();
    let mut planner_metrics = PlannerMetrics::default();
    let mut exec_metrics = ExecMetrics::default();
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
    let mut resume_iteration = telemetry::resume_iteration();
    let mut prev_bias: Option<policy::PolicyBias> = None;
    let mut last_signal_sig = String::new();
    let mut last_completed = 0usize;
    let mut stagnant_iters = 0u64;
    while !graph.all_completed() && iter < max_iterations {
        eprintln!("{}", console::phase("planner", &format!("iter={} nodes={}", iter, graph.nodes.len())));
        let iter_start = std::time::Instant::now();
        let completed_now = graph.nodes.iter().filter(|n| n.status == dag::Status::Completed).count();
        if completed_now <= last_completed {
            stagnant_iters = stagnant_iters.saturating_add(1);
        } else {
            stagnant_iters = 0;
        }
        last_completed = completed_now;
        let failure_stats = failure_store.stats();
        let features = graph_features(graph).with_failure_stats(&failure_stats);
        let policy_outcome = policy_engine::evaluate(&features, config.max_nodes, config.max_nodes.saturating_mul(4));
        let policy_bias = policy_outcome.bias.clone();
        let policy_decision = policy_outcome.decision.clone();
        let mut run_planner = policy_decision.run_planner;
        let mut expansion_scale = policy_decision.expansion_scale;
        let execution_preference = policy_decision.execution_preference;
        let current_sig = graph_signature(graph);
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
        let mut phase = PlannerPhase::ReuseTemplate;
        let mut reuse_decision = false;
        let mut reuse_score = 0.0;
        let mut reuse_goal: Option<String> = None;
        let mut reuse_goal_similarity = 0.0;
        let mut reuse_by_embedding = false;
        last_embedding_cache_hits = 0;
        if matches!(phase, PlannerPhase::ReuseTemplate) {
            if !run_planner {
                let search = store.find_similar(template_name, graph, config.template_top_k, config.goal_similarity_weight, config.structural_similarity_weight, config.embedding_dim);
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
                        if let Ok(loaded) = store.load(&best.entry.goal) {
                            *graph = loaded;
                            graph.reset_for_execution();
                            graph.rebuild_index();
                            reuse_decision = true;
                        }
                    }
                }
            }
            last_template_reuse = reuse_decision;
            last_template_score = reuse_score;
            last_template_selected = reuse_goal.clone();
            last_goal_similarity = reuse_goal_similarity;
            last_template_by_embedding = reuse_by_embedding && reuse_decision;
            if !reuse_decision && !run_planner {
                run_planner = true;
            }
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerEvent::ReuseDone as usize];
        }
        let recovery_reason = engine::take_recovery_signal(Path::new(LOG_ROOT));
        let mut rewrite_requests = rewrite_requests;
        if let Some(reason) = recovery_reason.as_ref() {
            for node in &graph.nodes {
                if matches!(node.status, dag::Status::Pending | dag::Status::Failed) {
                    rewrite_requests.push(node.id.clone());
                }
            }
            eprintln!("{}", console::info("recovery", &format!("reason={} rewrites={}", reason, rewrite_requests.len())));
        }
        let signals = compute_graph_signals(graph);
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

        if matches!(phase, PlannerPhase::MutateTemplate) && force_planner_expand && config.mutation_candidates > 0 {
            let base_reward = store.stored_reward(template_name);
            let candidates = template_mutation::generate_candidates(graph, config.mutation_candidates, config.mutation_budget, config.mutation_rate, iter);
            let mut scored = template_mutation::evaluate_candidates(candidates);
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
                if let Ok((_, _)) = execute_graph_loop(
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
                )
                .await
                {
                    let reward = telemetry::compute_reward(&eval_graph, 1, config.max_expand_iters as u64);
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
        if matches!(phase, PlannerPhase::MutateTemplate) {
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerEvent::MutationDone as usize];
        }
        if matches!(phase, PlannerPhase::PlannerUpdate) && run_planner_now {
            for attempt in 1..=attempts {
                planner_metrics.planner_calls += 1;
                if attempt > 1 {
                    planner_metrics.planner_retries += 1;
                }
                let mut features_for_bias = features.clone();
                if let Some(ctx) = planner.reward_context() {
                    features_for_bias = features_for_bias.with_reward_history(&ctx.recent_rewards);
                }
                let normalized = normalize_features(&features_for_bias, config.max_nodes, config.max_nodes.saturating_mul(4));
                let bias_raw = policy::PolicyModel::load_default().predict(&normalized);
                let bias_smoothed = policy::smooth_bias(prev_bias.as_ref(), bias_raw);
                let bias = policy::maybe_explore(bias_smoothed, 0.05);
                prev_bias = Some(bias.clone());
                let bias_text = policy::format_bias(&bias);

                let prompt = planner.build_prompt(
                    graph,
                    &signals,
                    &features,
                    &cost_table.summary(5, config.cost_latency_weight, config.cost_failure_weight),
                    &rewrite_requests,
                    &bias_text,
                    planner_max_new_nodes,
                    planner_max_new_edges,
                );
                let mut candidate = PlannerUpdate { new_nodes: Vec::new(), new_edges: Vec::new(), retract_nodes: Vec::new(), rewrite_nodes: Vec::new() };
                let attempts = retry_count.max(1);
                for attempt in 1..=attempts {
                    let allow_mismatch = attempt > 1 && planner.is_history_empty();
                    let raw = engine::call_llm_raw_with_retry_allow_mismatch(
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
                                let retry_raw = engine::call_llm_raw_with_retry_allow_mismatch(
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
                    match planner.apply_raw_response(raw, &log_dir, iter, graph.nodes.len(), &signals) {
                        Ok(update) => {
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
                let repaired = auto_repair_planner_update(graph, &mut candidate);
                if repaired.count > 0 {
                    let ids = repaired.ids.join(", ");
                    eprintln!("[planner] auto-repaired {} mixed-class nodes: [{}]", repaired.count, ids);
                }
                let output_payload = serde_json::json!({
                    "iter": iter,
                    "attempt": attempt,
                    "auto_repaired": repaired.count,
                    "auto_repair_ids": repaired.ids,
                    "planner_output": candidate,
                });
                let output_path = log_dir.join(format!("planner_iter_{:04}_output.json", iter));
                if let Ok(pretty) = serde_json::to_string_pretty(&output_payload) {
                    let _ = std::fs::write(output_path, pretty);
                }
                let mut candidate_graph = graph.clone();
                if let Err(e) = apply_planner_update(&mut candidate_graph, candidate.clone()) {
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                    planner_metrics.planner_failures += 1;
                    return Err(e);
                }
                let candidate_sig = graph_signature(&candidate_graph);
                if failure_store.contains(&candidate_sig) {
                    let payload = serde_json::json!({
                        "iter": iter,
                        "attempt": attempt,
                        "error": "planner candidate matches known failure signature",
                        "signature": candidate_sig,
                    });
                    let path = log_dir.join(format!("planner_iter_{:04}_rejected_failure.json", iter));
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        let _ = std::fs::write(path, pretty);
                    }
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                        continue;
                    }
                }
                if let Err(e) =
                    validate_planner_update(graph, &candidate, planner_max_new_nodes, planner_max_new_edges, &mut failure_store, iter, config.failure_constraint_threshold, config.max_constraints)
                {
                    let err_msg = e.to_string();
                    if err_msg.starts_with("constraint violated:") {
                        constraint_rejections += 1;
                        constraint_types.push(err_msg.replace("constraint violated: ", ""));
                    }
                    if err_msg.contains("cycle detected") || err_msg.contains("capability class") {
                        store.record_failure(&template_hash);
                    }
                    let payload = serde_json::json!({
                        "iter": iter,
                        "attempt": attempt,
                        "error": err_msg,
                        "planner_output": candidate,
                    });
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
                    let payload = serde_json::json!({
                        "iter": iter,
                        "attempt": attempt,
                        "error": "plateaued template requires expansion",
                        "planner_output": candidate,
                    });
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
            apply_planner_update(&mut updated, update)?;
            updated.validate().map_err(|e| anyhow::anyhow!(e))?;
            store.save(template_name, &updated)?;
            *graph = updated;
            eprintln!("[planner] applied update: nodes={} edges={}", graph.nodes.len(), edge_count(graph));
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
        if matches!(phase, PlannerPhase::PlannerUpdate) {
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerEvent::PlannerDone as usize];
        }
        let (exec_iters, exec_failures) = if matches!(phase, PlannerPhase::Execute) {
            let res = execute_graph_loop(
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
            )
            .await?;
            phase = PLANNER_TRANSITIONS[phase as usize][PlannerEvent::ExecuteDone as usize];
            res
        } else {
            (0, Vec::new())
        };
        if matches!(phase, PlannerPhase::Evaluate) {
            phase = PlannerPhase::ReuseTemplate;
        }
        for failure in exec_failures {
            failure_store.record_graph(failure.kind, graph, failure.iter);
            store.record_failure(&template_hash);
        }
        let _exec_iters = exec_iters;
        let iterations_used = iter.saturating_add(1);
        planner_metrics.iterations += 1;
        let reward = telemetry::compute_reward(graph, iterations_used, max_iterations);
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
        let runtime = RuntimeMetrics {
            queue_depth: telemetry::pending_requests(),
            retry_rate: if planner_metrics.planner_calls == 0 { 0.0 } else { planner_metrics.planner_retries as f64 / planner_metrics.planner_calls as f64 },
            progress_fraction: telemetry::progress_fraction(graph),
            iteration_time_ms: iter_start.elapsed().as_millis() as u64,
            branching_factor: features.branching_factor,
            blocked_fraction: features.blocked_fraction,
            completion_velocity: features.completion_velocity,
            policy_prediction,
            policy_error,
            policy_weight_norm: policy_outcome.weight_norm,
            dataset_size: policy_train::dataset_size(),
            deadlock_rate: features.deadlock_rate,
            policy_run_planner: run_planner_now,
            policy_expansion_scale: expansion_scale,
            policy_execution_preference: execution_preference,
            template_reuse: last_template_reuse,
            template_score: last_template_score,
            template_selected: last_template_selected.clone(),
            repair_attempts: exec_metrics.last_repair_attempts,
            repair_success_rate: if exec_metrics.last_repair_attempts == 0 { 0.0 } else { exec_metrics.last_repair_successes as f64 / exec_metrics.last_repair_attempts as f64 },
            repair_type: exec_metrics.last_repair_kind.clone(),
            constraint_rejections,
            constraint_hit_rate: if attempts == 0 { 0.0 } else { constraint_rejections as f64 / attempts as f64 },
            constraint_types: if constraint_types.is_empty() { None } else { Some(constraint_types.join(",")) },
            avg_capability_latency: cost_table.avg_latency(),
            avg_capability_failure: cost_table.avg_failure(),
            avg_node_utility,
            template_mutations: last_mutations,
            mutation_success_rate: if last_mutations == 0 { 0.0 } else { last_mutation_success as f64 / last_mutations as f64 },
            mutation_reward_delta: last_mutation_reward_delta,
            snapshot_written: exec_metrics.last_snapshot_written,
            snapshot_loaded: resume_iteration > 0,
            resume_iteration,
            goal_similarity_score: last_goal_similarity,
            template_reuse_by_embedding: last_template_by_embedding,
            embedding_cache_hits: last_embedding_cache_hits,
        };
        let reward_history = store.recent_rewards(template_name, 6);
        let features = features.with_reward_history(&reward_history);
        let failures = failure_store.failure_count();
        let (add_nodes, add_edges, rewrites) = last_update_counts.unwrap_or((0, 0, 0));
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
                "failures": failures
            }),
            action: serde_json::json!({
                "add_nodes": add_nodes,
                "add_edges": add_edges,
                "rewrites": rewrites
            }),
            policy_decision: serde_json::json!({
                "run_planner": run_planner,
                "expansion_scale": expansion_scale,
                "execution_preference": execution_preference
            }),
            reward,
        };
        policy_train::append_policy_dataset(&entry);
        policy_train::update_online(&entry, config.max_nodes, config.max_nodes.saturating_mul(4));
        if let Some(rewrites) = revision_rewrites {
            store.record_revision(template_name, graph, reward, rewrites, iter);
        }
        let snapshot = TelemetrySnapshot {
            planner: planner_metrics.clone(),
            exec: exec_metrics.clone(),
            runtime,
            reward,
            template_hash: Some(store.hash_for(template_name)),
            goal: Some(template_name.to_string()),
        };
        telemetry::record_all_snapshots(&snapshot, LOG_ROOT, TEMPLATE_ROOT, &template_hash);
        iter += 1;
        resume_iteration = iter;
    }
    let reward = telemetry::compute_reward(graph, iter, max_iterations);
    if graph.all_completed() && !graph.has_failed() {
        if let Err(e) = store.save_with_reward(template_name, graph, reward) {
            eprintln!("[templates] failed to persist updated template: {}", e);
        }
    }
    store.record_reward(template_name, reward);
    Ok(reward)
}
