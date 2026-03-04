use crate::ws_server::WsBridge;
use anyhow::Result;
use futures_util::future::join_all;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::io::Write;
use tokio::sync::Semaphore;

use super::config::{self, CapabilityConfig};
use super::capability::assert_class_disjoint;
use super::dag;
use super::engine;
use super::endpoint_scheduler;
use super::graph_algo::{emit_planned_graph, compute_graph_signals, run_graph_algorithms, graph_signature, node_utility, graph_features};
use super::graph_runtime::{build_context, enforce_semantic_validations, prune_unlinked_nodes};
use super::tab_management::{self, TabsHandle};
use super::LOG_ROOT;
use super::TEMPLATE_ROOT;
use super::planner_session::{PlannerSession, PlannerUpdate};
use super::templates::TemplateStore;
use super::failure_store::FailureStore;
use super::dag::TaskNode;
use super::telemetry::{self, ExecMetrics, PlannerMetrics, RuntimeMetrics, TelemetrySnapshot};
use super::console;
use super::capability::dominant_class;
use super::gpu_scheduler::driver::GpuScheduler;

#[derive(serde::Serialize)]
struct PolicyDatasetEntry {
    features: serde_json::Value,
    action: serde_json::Value,
    reward: f64,
}

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

#[derive(Serialize)]
struct TemplateRevisionLog {
    template_hash: String,
    reward: f64,
    nodes: usize,
    edges: usize,
    rewrites: usize,
}

fn edge_count(graph: &dag::TaskGraph) -> usize {
    graph.nodes.iter().map(|n| n.deps.len()).sum()
}

fn prune_low_value_nodes(graph: &mut dag::TaskGraph, iter: u64, config: &CapabilityConfig) {
    if !config.auto_prune {
        return;
    }
    let mut parents = std::collections::HashSet::new();
    for node in &graph.nodes {
        for dep in &node.deps {
            parents.insert(dep.clone());
        }
    }
    let mut pruned = Vec::new();
    for node in &graph.nodes {
        if node.status != dag::Status::Completed {
            continue;
        }
        if node.deps.is_empty() {
            continue;
        }
        if parents.contains(&node.id) {
            continue;
        }
        let age = node.completed_iter.map(|t| iter.saturating_sub(t)).unwrap_or(0);
        if age < config.prune_min_age {
            continue;
        }
        let util = node_utility(graph, &node.id, iter);
        if util < config.prune_threshold {
            pruned.push(node.id.clone());
        }
    }
    if pruned.is_empty() {
        return;
    }
    graph.nodes.retain(|n| !pruned.contains(&n.id));
    for node in &mut graph.nodes {
        node.deps.retain(|d| !pruned.contains(d));
    }
    graph.rebuild_index();
}

fn append_policy_dataset(entry: PolicyDatasetEntry) {
    let path = Path::new("/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl");
    if let Ok(line) = serde_json::to_string(&entry) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", line).as_bytes()));
    }
}

fn apply_recovery(graph: &mut dag::TaskGraph) {
    for node in &mut graph.nodes {
        if node.status == dag::Status::Failed {
            node.status = dag::Status::Pending;
            node.readonly_fail_count = 0;
            node.error = None;
            node.result = None;
        }
    }
    graph.rebuild_index();
}

pub(crate) async fn execute_graph_loop(
    graph: &mut dag::TaskGraph,
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &TabsHandle,
    cwd: &[PathBuf],
    workspace_listing: &str,
    endpoint: &config::LlmEndpoint,
    exec_role: &str,
    policy: &config::CapabilityPolicy,
    context_radius: usize,
    max_concurrency: usize,
    max_iterations: u64,
    tab_cooldown_ms: u64,
    retry_count: u32,
    retry_delay: u64,
    max_output_lines: usize,
    exec_metrics: &mut ExecMetrics,
) -> Result<(u64, Vec<ExecFailure>)> {
    let semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
    let mut blocked_streak = 0u32;
    let mut state = PipelineState::Running;
    let mut failures = Vec::new();
    for iter in 1..=max_iterations {
        let features = graph_features(graph);
        let mut ready_ids = GpuScheduler::schedule(graph);
        for id in &ready_ids {
            let _ = graph.update_status(id, dag::Status::Ready);
        }
        let status_snapshot = graph
            .nodes
            .iter()
            .map(|n| serde_json::json!({"id": n.id, "status": n.status}))
            .collect::<Vec<_>>();
        if ready_ids.is_empty() && !graph.all_completed() && !graph.has_failed() {
            if GpuScheduler::detect_deadlock(graph) {
                failures.push(ExecFailure { kind: "deadlock", iter });
            }
        }
        if graph.nodes.iter().any(|n| n.readonly_fail_count > policy.max_node_retries) {
            failures.push(ExecFailure { kind: "verify_loop", iter });
        }
        let summary = serde_json::json!({
            "iter": iter,
            "ready": ready_ids,
            "status": status_snapshot
        });
        let status_path = Path::new(LOG_ROOT).join(format!("iter_{:03}_status.json", iter));
        let _ = std::fs::write(
            status_path,
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        );
        eprintln!("{}", console::info("capability", &summary.to_string()));
        let completed_count = graph
            .nodes
            .iter()
            .filter(|n| n.status == dag::Status::Completed)
            .count();
        let failed_count = graph.nodes.iter().filter(|n| n.status == dag::Status::Failed).count();
        eprintln!(
            "{}",
            console::phase(
                "tick",
                &format!(
                    "iter={} ready={} completed={}/{} failed={}",
                    iter,
                    ready_ids.len(),
                    completed_count,
                    graph.nodes.len(),
                    failed_count
                )
            )
        );

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
                eprintln!(
                    r#"[capability] {{"iter":{},"event":"blocked","streak":{}}}"#,
                    iter,
                    blocked_streak
                );
                if blocked_streak >= 3 {
                    anyhow::bail!("blocked");
                }
                continue;
            }
            _ => blocked_streak = 0,
        }

        if features.branching_factor > 3.5 {
            let keep = (ready_ids.len() / 2).max(1);
            ready_ids.truncate(keep);
        }
        ready_ids.sort_by_key(|id| {
            let node = graph.nodes.iter().find(|n| n.id == *id);
            let base = node.map(|n| n.priority as i32).unwrap_or(0);
            let retry_penalty = node.map(|n| n.readonly_fail_count as i32).unwrap_or(0);
            let unblock_bonus = if features.blocked_fraction > 0.4 {
                node.map(|n| n.required_capabilities.iter().any(|c| c.class() == super::capability::CapabilityClass::Observe))
                    .unwrap_or(false) as i32
            } else { 0 };
            let completion_bonus = features.completion_velocity.min(5.0) as i32;
            let adjusted = base + completion_bonus + unblock_bonus - retry_penalty;
            std::cmp::Reverse(adjusted)
        });

        let mut futures = Vec::new();
        for node_id in ready_ids {
            let node = match graph.get_node(&node_id).cloned() {
                Some(n) => n,
                None => continue,
            };
            let auth = match dag::grant_authority(&node) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!(
                        r#"[capability] {{"iter":{},"event":"authority_error","node":"{}","error":"{}"}}"#,
                        iter,
                        node.id,
                        e
                    );
                    let _ = graph.update_status(&node.id, dag::Status::Ready);
                    continue;
                }
            };
            let sem = semaphore.clone();
            let selected = endpoint_scheduler::select_endpoints_for_role(config, role_rr, exec_role, 1).await;
            let exec_ep = selected.get(0).map(|e| (e.id.clone(), e.url.clone(), e.max_tabs, None))
                .unwrap_or_else(|| (endpoint.id.clone(), endpoint.url.clone(), endpoint.max_tabs, Some(endpoint.stateful)));
            let endpoint_id = exec_ep.0;
            let url = exec_ep.1;
            let max_tabs = exec_ep.2;
            let stateful = exec_ep.3.unwrap_or(endpoint.stateful);
            let workspace_root = cwd[0].clone();
            let log_dir = Path::new(LOG_ROOT).to_path_buf();
            let node_id = node.id.clone();
            let context = build_context(graph, &node.id, context_radius);
            let node_type_str = format!("{:?}", node.node_type).to_lowercase();
            let caps_str = node
                .required_capabilities
                .iter()
                .map(|c| format!("{:?}", c).to_lowercase())
                .collect::<Vec<_>>()
                .join(",");
            let mode = dominant_class(&node.required_capabilities);
            eprintln!(
                "{}",
                console::info(
                    "dispatch",
                    &format!(
                        "node={} type={} mode={} caps=[{}] endpoint={}",
                        node.id,
                        node_type_str,
                        console::mode_tag(mode),
                        caps_str,
                        endpoint_id
                    )
                )
            );
            let fut = async move {
                let start = std::time::Instant::now();
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| anyhow::anyhow!("semaphore closed"))?;
                let res = engine::call_node(
                    &node,
                    &auth,
                    bridge,
                    &endpoint_id,
                    &url,
                    stateful,
                    "",
                    tabs,
                    max_tabs,
                    tab_cooldown_ms,
                    &workspace_root,
                    &context,
                    &log_dir,
                    iter,
                    retry_count,
                    retry_delay,
                )
                .await;
                Ok::<_, anyhow::Error>((node_id, res, start.elapsed()))
            };
            futures.push(fut);
        }

        let results = join_all(futures).await;
        let mut total_ms = 0u128;
        let mut count = 0u64;
        for item in results {
            if let Some(ms) = process_node_result(
                item,
                graph,
                cwd,
                max_output_lines,
                iter,
                policy,
                exec_metrics,
            ) {
                count += 1;
                total_ms = total_ms.saturating_add(ms);
            }
        }
        exec_metrics.nodes_executed = exec_metrics.nodes_executed.saturating_add(count);
        if count > 0 {
            let avg = (total_ms / count as u128) as u64;
            exec_metrics.avg_latency_ms = update_avg(exec_metrics.avg_latency_ms, avg);
        }

        super::ensure_unique_node_ids(&mut graph.nodes);
        let iter_u32 = u32::try_from(iter).unwrap_or(u32::MAX);
        emit_planned_graph(graph, Path::new(LOG_ROOT), iter_u32);
        run_graph_algorithms(graph, Path::new(LOG_ROOT), iter_u32);

        if config.prune_unlinked {
            prune_unlinked_nodes(graph);
        }
        enforce_semantic_validations(graph)?;
        prune_low_value_nodes(graph, iter, config);
        if features.retry_rate > config.recovery_retry_rate_threshold
            || features.failed_fraction > config.recovery_failed_fraction_threshold
        {
            apply_recovery(graph);
        }
    }
    anyhow::bail!("iteration limit exceeded")
}

fn process_node_result(
    item: Result<(String, Result<engine::NodeCallResult>, std::time::Duration)>,
    graph: &mut dag::TaskGraph,
    cwd: &[PathBuf],
    max_output_lines: usize,
    iter: u64,
    policy: &config::CapabilityPolicy,
    exec_metrics: &mut ExecMetrics,
) -> Option<u128> {
    let (node_id, call_result, elapsed) = match item {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                r#"[capability] {{"iter":{},"event":"join_error","error":"{}"}}"#,
                iter,
                e
            );
            return None;
        }
    };
    let ms = elapsed.as_millis();
    let outcome = call_result.and_then(|r| engine::apply_node_result(
        r,
        graph,
        cwd,
        max_output_lines,
        Path::new(LOG_ROOT),
        iter,
        policy,
    ));
    if let Err(e) = outcome {
        eprintln!(
            r#"[capability] {{"iter":{},"event":"call_or_apply_error","node":"{}","error":"{}"}}"#,
            iter,
            node_id,
            e
        );
        let _ = graph.update_status(&node_id, dag::Status::Ready);
        exec_metrics.nodes_failed += 1;
    }
    if let Some(n) = graph.get_node_mut(&node_id) {
        if n.readonly_fail_count > policy.max_node_retries {
            n.readonly_fail_count = 0;
            n.status = dag::Status::Pending;
            n.error = None;
            n.result = None;
        }
    }
    Some(ms)
}

fn update_avg(current: u64, next: u64) -> u64 {
    current.checked_add(next).map(|s| s / 2).unwrap_or(next)
}

pub(crate) async fn run_planner_execution_loop(
    planner: &mut PlannerSession,
    graph: &mut dag::TaskGraph,
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &TabsHandle,
    cwd: &[PathBuf],
    workspace_listing: &str,
    endpoint: &config::LlmEndpoint,
    exec_role: &str,
    policy: &config::CapabilityPolicy,
    context_radius: usize,
    max_concurrency: usize,
    max_iterations: u64,
    tab_cooldown_ms: u64,
    retry_count: u32,
    retry_delay: u64,
    max_output_lines: usize,
    store: &mut TemplateStore,
    template_name: &str,
) -> Result<f64> {
    let template_hash = store.hash_for(template_name);
    let mut failure_store = FailureStore::load(&template_hash);
    let mut planner_metrics = PlannerMetrics::default();
    let mut exec_metrics = ExecMetrics::default();
    let mut empty_streak = 0u32;
    let mut iter = 0u64;
    while !graph.all_completed() && iter < max_iterations {
        eprintln!("{}", console::phase("planner", &format!("iter={} nodes={}", iter, graph.nodes.len())));
        let iter_start = std::time::Instant::now();
        let signals = compute_graph_signals(graph);
        let log_dir = Path::new(LOG_ROOT).join("planner_logs");
        let mut revision_rewrites = None;
        let mut last_update_counts = None;
        let mut update = None;
        let attempts = retry_count.max(1);
        let force_planner_expand = store.is_plateaued(
            template_name,
            config.planner_plateau_window,
            config.planner_plateau_threshold,
        );
        let remaining_nodes = config.max_nodes.saturating_sub(graph.nodes.len());
        let (planner_max_new_nodes, planner_max_new_edges) = if force_planner_expand {
            (
                (config.planner_max_new_nodes.saturating_mul(config.planner_plateau_expand_factor))
                    .min(remaining_nodes.max(1)),
                config.planner_max_new_edges.saturating_mul(config.planner_plateau_expand_factor),
            )
        } else {
            (config.planner_max_new_nodes, config.planner_max_new_edges)
        };
        for attempt in 1..=attempts {
            planner_metrics.planner_calls += 1;
            if attempt > 1 {
                planner_metrics.planner_retries += 1;
            }
            let candidate = planner
                .planner_iteration(
                    graph,
                    &signals,
                    bridge,
                    tabs,
                    endpoint.max_tabs,
                    tab_cooldown_ms,
                    retry_count,
                    retry_delay,
                    &log_dir,
                    iter,
                    planner_max_new_nodes,
                    planner_max_new_edges,
                )
                .await?;
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
            if let Err(e) = validate_planner_update(
                graph,
                &candidate,
                planner_max_new_nodes,
                planner_max_new_edges,
                &mut failure_store,
                iter,
            ) {
                let err_msg = e.to_string();
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
        if let Some(update) = update {
            revision_rewrites = Some(update.rewrite_nodes.len());
            last_update_counts = Some((
                update.new_nodes.len(),
                update.new_edges.len(),
                update.rewrite_nodes.len(),
            ));
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
            store.update(template_name, update)?;
            *graph = store.load(template_name)?;
        }
        let (exec_iters, exec_failures) = execute_graph_loop(
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
            &mut exec_metrics,
        )
        .await?;
        for failure in exec_failures {
            failure_store.record_graph(failure.kind, graph, failure.iter);
            store.record_failure(&template_hash);
        }
        let _exec_iters = exec_iters;
        let iterations_used = iter.saturating_add(1);
        planner_metrics.iterations += 1;
        let features = graph_features(graph);
        let runtime = RuntimeMetrics {
            queue_depth: telemetry::pending_requests(),
            retry_rate: if planner_metrics.planner_calls == 0 {
                0.0
            } else {
                planner_metrics.planner_retries as f64 / planner_metrics.planner_calls as f64
            },
            progress_fraction: telemetry::progress_fraction(graph),
            iteration_time_ms: iter_start.elapsed().as_millis() as u64,
            branching_factor: features.branching_factor,
            blocked_fraction: features.blocked_fraction,
            completion_velocity: features.completion_velocity,
        };
        let reward = telemetry::compute_reward(graph, iterations_used, max_iterations);
        let reward_history = store.recent_rewards(template_name, 6);
        let features = features.with_reward_history(&reward_history);
        let failures = failure_store.failure_count();
        let (add_nodes, add_edges, rewrites) = last_update_counts.unwrap_or((0, 0, 0));
        append_policy_dataset(PolicyDatasetEntry {
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
                "failures": failures
            }),
            action: serde_json::json!({
                "add_nodes": add_nodes,
                "add_edges": add_edges,
                "rewrites": rewrites
            }),
            reward,
        });
        if let Some(rewrites) = revision_rewrites {
            let revision = TemplateRevisionLog {
                template_hash: store.hash_for(template_name),
                reward,
                nodes: graph.nodes.len(),
                edges: edge_count(graph),
                rewrites,
            };
            let path = Path::new(TEMPLATE_ROOT).join(format!("template_revision_{:04}.json", iter));
            if let Ok(pretty) = serde_json::to_string_pretty(&revision) {
                let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
                let _ = std::fs::write(path, pretty);
            }
        }
        let snapshot = TelemetrySnapshot {
            planner: planner_metrics.clone(),
            exec: exec_metrics.clone(),
            runtime,
            reward,
            template_hash: Some(store.hash_for(template_name)),
            goal: Some(template_name.to_string()),
        };
        telemetry::record_snapshot(&Path::new(LOG_ROOT).join("planner_logs/metrics.json"), &snapshot);
        telemetry::record_snapshot(&Path::new(LOG_ROOT).join("metrics.json"), &snapshot);
        let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
        telemetry::record_snapshot(
            &Path::new(TEMPLATE_ROOT).join(format!("metrics_{}.json", template_hash)),
            &snapshot,
        );
        iter += 1;
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

fn validate_planner_update(
    graph: &dag::TaskGraph,
    update: &PlannerUpdate,
    planner_max_new_nodes: usize,
    planner_max_new_edges: usize,
    failure_store: &mut FailureStore,
    iteration: u64,
) -> Result<()> {
    ensure(update.new_nodes.len() <= planner_max_new_nodes, "planner expansion limit exceeded")?;
    ensure(update.new_edges.len() <= planner_max_new_edges, "planner edge limit exceeded")?;

    let mut existing: HashMap<String, usize> = HashMap::new();
    let mut status_by_id: HashMap<String, dag::Status> = HashMap::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        existing.insert(node.id.clone(), idx);
        status_by_id.insert(node.id.clone(), node.status);
    }
    let mut new_ids: HashMap<String, usize> = HashMap::new();
    update.new_nodes.iter().try_for_each(|spec| {
        ensure(!spec.id.trim().is_empty(), "planner node id empty")?;
        ensure(!spec.description.trim().is_empty(), "planner node description empty")?;
        ensure(!(existing.contains_key(&spec.id) || new_ids.contains_key(&spec.id)),
               &format!("duplicate node id {}", spec.id))?;
        new_ids.insert(spec.id.clone(), new_ids.len());
        Ok::<(), anyhow::Error>(())
    })?;

    update.new_edges.iter().try_for_each(|edge| {
        ensure(!edge.from.trim().is_empty() && !edge.to.trim().is_empty(), "planner edge endpoints empty")?;
        let from_ok = existing.contains_key(&edge.from) || new_ids.contains_key(&edge.from);
        let to_ok = existing.contains_key(&edge.to) || new_ids.contains_key(&edge.to);
        ensure(from_ok && to_ok, "planner edge references unknown node")
    })?;

    update.retract_nodes.iter().try_for_each(|spec| {
        let status = status_by_id.get(&spec.id).copied()
            .ok_or_else(|| anyhow::anyhow!("retract references unknown node"))?;
        ensure(matches!(status, dag::Status::Pending | dag::Status::Failed), "retract node must be pending or failed")
    })?;

    update.rewrite_nodes.iter().try_for_each(|spec| {
        let status = status_by_id.get(&spec.id).copied()
            .ok_or_else(|| anyhow::anyhow!("rewrite references unknown node"))?;
        ensure(matches!(status, dag::Status::Pending), "rewrite node must be pending")?;
        let caps: std::collections::HashSet<_> = spec.new_capabilities.iter().copied().collect();
        assert_class_disjoint(&caps).map_err(|e| anyhow::anyhow!(e))
    })?;

    let mut test_graph = graph.clone();
    apply_planner_update(&mut test_graph, update.clone())?;
    if let Err(e) = test_graph.validate() {
        let msg = e.to_string();
        if msg.contains("cycle detected") {
            failure_store.record_graph("cycle", &test_graph, iteration);
        } else if msg.contains("capability class") {
            failure_store.record_graph("invalid_authority", &test_graph, iteration);
        }
        return Err(anyhow::anyhow!(e));
    }
    Ok(())
}

pub(crate) fn apply_planner_update(graph: &mut dag::TaskGraph, update: PlannerUpdate) -> Result<()> {
    let retract_ids: std::collections::HashSet<String> = update.retract_nodes.into_iter()
        .filter_map(|spec| {
            graph.nodes.iter()
                .find(|n| n.id == spec.id)
                .filter(|n| matches!(n.status, dag::Status::Pending | dag::Status::Failed))
                .map(|_| spec.id)
        })
        .collect();

    if !retract_ids.is_empty() {
        graph.nodes.retain(|n| !retract_ids.contains(&n.id));
        for node in &mut graph.nodes {
            node.deps.retain(|d| !retract_ids.contains(d));
        }
        graph.rebuild_index();
    }

    for spec in update.rewrite_nodes {
        if let Some(node) = graph.get_node_mut(&spec.id) {
            if node.status == dag::Status::Pending {
                let caps: std::collections::HashSet<_> = spec.new_capabilities.iter().copied().collect();
                assert_class_disjoint(&caps).map_err(|e| anyhow::anyhow!(e))?;
                node.description = spec.new_description;
                node.required_capabilities = spec.new_capabilities;
            }
        }
    }

    let existing: std::collections::HashSet<String> =
        graph.nodes.iter().map(|n| n.id.clone()).collect();

    graph.nodes.extend(
        update.new_nodes.into_iter()
            .filter(|s| !existing.contains(&s.id))
            .map(|spec| TaskNode {
                id: spec.id,
                description: spec.description,
                status: dag::Status::Pending,
                deps: spec.deps,
                required_capabilities: spec.required_capabilities,
                node_type: spec.node_type,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: spec.reasoning_trace,
                result: None,
                error: None,
                readonly_fail_count: 0,
                completed_iter: None,
            })
    );

    let id_to_idx: HashMap<String, usize> =
        graph.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();

    for edge in update.new_edges {
        if let Some(&to_idx) = id_to_idx.get(&edge.to) {
            let deps = &mut graph.nodes[to_idx].deps;
            if !deps.contains(&edge.from) { deps.push(edge.from); }
        }
    }
    Ok(())
}

fn ensure(cond: bool, msg: &str) -> Result<()> {
    cond.then_some(()).ok_or_else(|| anyhow::anyhow!("{}", msg))
}
