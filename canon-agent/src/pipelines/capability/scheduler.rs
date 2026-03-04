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
use super::capability::{assert_class_disjoint, Capability, CapabilityClass};
use super::dag;
use super::engine;
use super::dispatch;
use super::graph_algo::{compute_graph_signals, graph_signature, node_utility, graph_features};
use super::graph_maintenance::{self, MaintenanceCtx};
use super::graph_runtime::build_context;
use super::tab_management::{self, TabsHandle};
use super::LOG_ROOT;
use super::TEMPLATE_ROOT;
use super::planner_session::{PlannerSession, PlannerUpdate, EdgeSpec};
use super::templates::TemplateStore;
use super::failure_store::FailureStore;
use super::policy_train::{self, PolicyDatasetEntry};
use super::dag::TaskNode;
use super::telemetry::{self, ExecMetrics, PlannerMetrics, RuntimeMetrics, TelemetrySnapshot};
use super::console;
use super::capability::dominant_class;
use super::gpu_scheduler::driver::GpuScheduler;
use super::decompose;
use super::capability_cost::CapabilityCostTable;
use super::template_mutation;
use super::state_snapshot;
use super::scheduler_state::{ExecEvent, ExecStep, EXEC_TRANSITIONS};
use super::policy_engine;
use super::planner_state::{PlannerPhase, PlannerEvent, PLANNER_TRANSITIONS};

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

#[derive(Default)]
struct RepairStats {
    attempts: u64,
    successes: u64,
    last_kind: Option<String>,
}

fn edge_count(graph: &dag::TaskGraph) -> usize {
    graph.nodes.iter().map(|n| n.deps.len()).sum()
}

pub(crate) fn prune_low_value_nodes(graph: &mut dag::TaskGraph, iter: u64, config: &CapabilityConfig) {
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

pub(crate) fn apply_recovery(graph: &mut dag::TaskGraph) {
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

fn repair_node(
    graph: &mut dag::TaskGraph,
    node_id: &str,
    policy: &config::CapabilityPolicy,
    repair_radius: usize,
    max_repairs: u32,
) -> Option<String> {
    let node = graph.get_node_mut(node_id)?;
    if node.repair_attempts >= max_repairs {
        return None;
    }
    node.repair_attempts += 1;
    drop(node);

    let rules: &[fn(&mut dag::TaskGraph, &str, &config::CapabilityPolicy, usize) -> Option<&'static str>] = &[
        rule_retry,
        rule_capability_downgrade,
        rule_dependency_rewire,
        rule_node_split,
    ];

    for rule in rules {
        if let Some(kind) = rule(graph, node_id, policy, repair_radius) {
            return Some(kind.to_string());
        }
    }

    None
}

fn rule_retry(
    graph: &mut dag::TaskGraph,
    node_id: &str,
    policy: &config::CapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node.readonly_fail_count < policy.max_node_retries {
        node.status = dag::Status::Ready;
        node.error = None;
        node.result = None;
        return Some("retry");
    }
    None
}

fn rule_capability_downgrade(
    graph: &mut dag::TaskGraph,
    node_id: &str,
    _policy: &config::CapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node = graph.get_node_mut(node_id)?;
    if node
        .required_capabilities
        .iter()
        .any(|c| c.class() == super::capability::CapabilityClass::Mutate)
    {
        node.required_capabilities = vec![super::capability::Capability::FileRead];
        node.status = dag::Status::Pending;
        node.error = None;
        node.result = None;
        return Some("capability_downgrade");
    }
    None
}

fn rule_dependency_rewire(
    graph: &mut dag::TaskGraph,
    node_id: &str,
    _policy: &config::CapabilityPolicy,
    repair_radius: usize,
) -> Option<&'static str> {
    if repair_radius < 1 {
        return None;
    }
    let failed_deps: std::collections::HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.status == dag::Status::Failed)
        .map(|n| n.id.clone())
        .collect();
    let node = graph.get_node_mut(node_id)?;
    let before = node.deps.len();
    node.deps.retain(|dep| !failed_deps.contains(dep));
    if node.deps.len() != before {
        node.status = dag::Status::Pending;
        return Some("dependency_rewire");
    }
    None
}

fn rule_node_split(
    graph: &mut dag::TaskGraph,
    node_id: &str,
    _policy: &config::CapabilityPolicy,
    _repair_radius: usize,
) -> Option<&'static str> {
    let node_snapshot = graph.get_node(node_id).cloned()?;
    if node_snapshot.description.len() <= 120 {
        return None;
    }
    let new_id = format!("{}_analysis", node_snapshot.id);
    if graph.get_node(&new_id).is_some() {
        return None;
    }
    let new_node = TaskNode {
        id: new_id.clone(),
        description: format!("Analyze: {}", node_snapshot.description),
        status: dag::Status::Pending,
        deps: node_snapshot.deps.clone(),
        required_capabilities: vec![super::capability::Capability::ReadDag],
        node_type: decompose::NodeType::Analysis,
        priority: node_snapshot.priority,
        budget: node_snapshot.budget,
        reasoning_trace: None,
        result: None,
        error: None,
        readonly_fail_count: 0,
        repair_attempts: 0,
        completed_iter: None,
    };
    if let Some(node) = graph.get_node_mut(node_id) {
        node.deps = vec![new_id.clone()];
    }
    graph.nodes.push(new_node);
    graph.rebuild_index();
    Some("node_split")
}

fn take_recovery_signal() -> Option<String> {
    let path = Path::new(LOG_ROOT).join("recovery_signal.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("reason").and_then(|r| r.as_str()).map(|s| s.to_string()))
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
    execution_preference: f64,
    cost_table: &mut CapabilityCostTable,
    exec_metrics: &mut ExecMetrics,
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
                        "ready": ready_ids.clone(),
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
                                r#"[capability] {{\"iter\":{},\"event\":\"blocked\",\"streak\":{}}}"#,
                                iter,
                                blocked_streak
                            );
                            if blocked_streak >= 3 {
                                anyhow::bail!("blocked");
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
                    ready_ids.sort_by(|a, b| {
                        let na = graph.nodes.iter().find(|n| n.id == *a);
                        let nb = graph.nodes.iter().find(|n| n.id == *b);
                        let sa = na.map(|n| score_node(n, &features, cost_table, execution_preference, config)).unwrap_or(0.0);
                        let sb = nb.map(|n| score_node(n, &features, cost_table, execution_preference, config)).unwrap_or(0.0);
                        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    let mut futures = Vec::new();
                    for node_id in ready_ids.clone() {
                        let node = match graph.get_node(&node_id).cloned() {
                            Some(n) => n,
                            None => continue,
                        };
                        let auth = match dag::grant_authority(&node) {
                            Ok(a) => a,
                            Err(e) => {
                                eprintln!(
                                    r#"[capability] {{\"iter\":{},\"event\":\"authority_error\",\"node\":\"{}\",\"error\":\"{}\"}}"#,
                                    iter,
                                    node.id,
                                    e
                                );
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
                        ).await;
                        let mode = dominant_class(&node.required_capabilities);
                        let mode_label = console::mode_tag(mode);
                        dispatch::log_dispatch(&node, &mode_label, &ctx.endpoint_id);
                        let context = build_context(graph, &node.id, context_radius);
                        let fut = dispatch::dispatch_node_call(
                            node,
                            auth,
                            bridge,
                            tabs,
                            sem,
                            ctx,
                            context,
                            iter,
                            retry_count,
                            retry_delay,
                            tab_cooldown_ms,
                        );
                        futures.push(fut);
                    }
                    results = join_all(futures).await;
                }
                ExecStep::ApplyResults => {
                    let mut total_ms = 0u128;
                    let mut count = 0u64;
                    for item in results.drain(..) {
                        if let Some(ms) = process_node_result(
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
                        exec_metrics.avg_latency_ms = update_avg(exec_metrics.avg_latency_ms, avg);
                    }
                }
                ExecStep::MaintainGraph => {
                    graph_maintenance::maintain_graph(MaintenanceCtx {
                        graph,
                        config,
                        iter,
                        features_retry_rate: features.retry_rate,
                        features_failed_fraction: features.failed_fraction,
                        cost_table,
                    })?;
                    if config.enable_resume
                        && config.snapshot_interval_iters > 0
                        && iter % config.snapshot_interval_iters == 0
                    {
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
    anyhow::bail!("iteration limit exceeded")
}

fn score_node(
    node: &dag::TaskNode,
    features: &super::graph_algo::FeatureVector,
    cost_table: &CapabilityCostTable,
    execution_preference: f64,
    config: &CapabilityConfig,
) -> f64 {
    let base = node.priority as f64;
    let completion = features.completion_velocity;
    let unblock = node
        .required_capabilities
        .iter()
        .any(|c| c.class() == super::capability::CapabilityClass::Observe) as u8 as f64;
    let retry = node.readonly_fail_count as f64;
    let cost = cost_table.node_cost(
        &node.required_capabilities,
        config.cost_latency_weight,
        config.cost_failure_weight,
    );
    let w1 = 1.0;
    let w2 = 0.4;
    let w3 = 0.6;
    let w4 = 0.8;
    let w5 = 1.0;
    (w1 * base * (1.0 + execution_preference))
        + (w2 * completion)
        + (w3 * unblock)
        - (w4 * retry)
        - (w5 * cost)
}

fn process_node_result(
    item: Result<(String, Result<engine::NodeCallResult>, std::time::Duration)>,
    graph: &mut dag::TaskGraph,
    cwd: &[PathBuf],
    max_output_lines: usize,
    iter: u64,
    policy: &config::CapabilityPolicy,
    exec_metrics: &mut ExecMetrics,
    repair_stats: &mut RepairStats,
    repair_radius: usize,
    max_repairs: u32,
    cost_table: &mut CapabilityCostTable,
    cost_decay_rate: f64,
    cost_latency_weight: f64,
    cost_failure_weight: f64,
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
        if let Some(kind) = repair_node(graph, &node_id, policy, repair_radius, max_repairs) {
            repair_stats.attempts += 1;
            repair_stats.successes += 1;
            repair_stats.last_kind = Some(kind);
        } else {
            let _ = graph.update_status(&node_id, dag::Status::Failed);
            if let Some(n) = graph.get_node_mut(&node_id) {
                n.error = Some(e.to_string());
            }
            repair_stats.attempts += 1;
            repair_stats.last_kind = Some("repair_failed".to_string());
        }
        exec_metrics.nodes_failed += 1;
    }
    if let Some(n) = graph.get_node_mut(&node_id) {
        let success = matches!(n.status, dag::Status::Completed);
        let latency_ms = ms as f64;
        for cap in &n.required_capabilities {
            cost_table.update(*cap, latency_ms, success, cost_decay_rate);
        }
        let _node_cost = cost_table.node_cost(&n.required_capabilities, cost_latency_weight, cost_failure_weight);
        if n.readonly_fail_count > policy.max_node_retries {
            n.reasoning_trace = Some(format!(
                "REWRITE_REQUESTED: readonly failures exceeded {}",
                policy.max_node_retries
            ));
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
    while !graph.all_completed() && iter < max_iterations {
        eprintln!("{}", console::phase("planner", &format!("iter={} nodes={}", iter, graph.nodes.len())));
        let iter_start = std::time::Instant::now();
        let failure_stats = failure_store.stats();
        let features = graph_features(graph).with_failure_stats(&failure_stats);
        let policy_outcome = policy_engine::evaluate(&features, config);
        let policy_bias = policy_outcome.bias.clone();
        let policy_decision = policy_outcome.decision.clone();
        let mut run_planner = policy_decision.run_planner;
        let mut expansion_scale = policy_decision.expansion_scale;
        let execution_preference = policy_decision.execution_preference;
        if graph.nodes.is_empty() || features.deadlock_rate > 0.0 {
            run_planner = true;
        }
        if expansion_scale.is_nan() || expansion_scale <= 0.0 {
            expansion_scale = 1.0;
        }
        let rewrite_requests = graph
            .nodes
            .iter()
            .filter_map(|n| {
                n.reasoning_trace.as_ref().and_then(|trace| {
                    trace.starts_with("REWRITE_REQUESTED").then(|| n.id.clone())
                })
            })
            .collect::<Vec<_>>();
        let mut phase = PlannerPhase::ReuseTemplate;
        let mut reuse_decision = false;
        let mut reuse_score = 0.0;
        let mut reuse_goal: Option<String> = None;
        let mut reuse_goal_similarity = 0.0;
        let mut reuse_by_embedding = false;
        last_embedding_cache_hits = 0;
        if matches!(phase, PlannerPhase::ReuseTemplate) {
            if !run_planner {
                let search = store.find_similar(
                    template_name,
                    graph,
                    config.template_top_k,
                    config.goal_similarity_weight,
                    config.structural_similarity_weight,
                    config.embedding_dim,
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
        let recovery_reason = take_recovery_signal();
        let mut rewrite_requests = rewrite_requests;
        if let Some(reason) = recovery_reason.as_ref() {
            for node in &graph.nodes {
                if matches!(node.status, dag::Status::Pending | dag::Status::Failed) {
                    rewrite_requests.push(node.id.clone());
                }
            }
            eprintln!(
                "{}",
                console::info("recovery", &format!("reason={} rewrites={}", reason, rewrite_requests.len()))
            );
        }
        let signals = compute_graph_signals(graph);
        let log_dir = Path::new(LOG_ROOT).join("planner_logs");
        let mut revision_rewrites = None;
        let mut last_update_counts = None;
        let mut update = None;
        let mut constraint_rejections = 0u64;
        let mut constraint_types: Vec<String> = Vec::new();
        let attempts = retry_count.max(1);
        let force_planner_expand = store.is_plateaued(
            template_name,
            config.planner_plateau_window,
            config.planner_plateau_threshold,
        ) || features.ready_fraction < 0.1 || recovery_reason.is_some();
        let remaining_nodes = config.max_nodes.saturating_sub(graph.nodes.len());
        let (mut planner_max_new_nodes, mut planner_max_new_edges) = if force_planner_expand {
            (
                (config.planner_max_new_nodes.saturating_mul(config.planner_plateau_expand_factor))
                    .min(remaining_nodes.max(1)),
                config.planner_max_new_edges.saturating_mul(config.planner_plateau_expand_factor),
            )
        } else {
            (config.planner_max_new_nodes, config.planner_max_new_edges)
        };
        let run_planner_now = run_planner && !reuse_decision;
        if run_planner_now {
            planner_max_new_nodes = ((planner_max_new_nodes as f64) * expansion_scale)
                .round()
                .max(1.0)
                .min(remaining_nodes.max(1) as f64) as usize;
            planner_max_new_edges = ((planner_max_new_edges as f64) * expansion_scale)
                .round()
                .max(1.0) as usize;
        }

        if matches!(phase, PlannerPhase::MutateTemplate) && force_planner_expand && config.mutation_candidates > 0 {
            let base_reward = store.stored_reward(template_name);
            let candidates = template_mutation::generate_candidates(
                graph,
                config.mutation_candidates,
                config.mutation_budget,
                config.mutation_rate,
                iter,
            );
            last_mutations = candidates.len() as u64;
            let mut best_reward = base_reward;
            let mut best_graph = None;
            let mut success = 0u64;
            for candidate in candidates {
                if candidate.validate().is_err() {
                    continue;
                }
                let mut eval_graph = candidate.clone();
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
                ).await {
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
                let mut candidate = planner
                    .planner_iteration(
                        graph,
                        &signals,
                        &features,
                        &cost_table.summary(5, config.cost_latency_weight, config.cost_failure_weight),
                        &rewrite_requests,
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
                        config.max_nodes,
                        config.max_nodes.saturating_mul(4),
                    )
                    .await?;
                let repaired = auto_repair_planner_update(graph, &mut candidate);
                if repaired > 0 {
                    eprintln!("[planner] auto-repaired {} mixed-class nodes", repaired);
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
                if let Err(e) = validate_planner_update(
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
            repair_success_rate: if exec_metrics.last_repair_attempts == 0 {
                0.0
            } else {
                exec_metrics.last_repair_successes as f64 / exec_metrics.last_repair_attempts as f64
            },
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
        append_policy_dataset(entry.clone());
        policy_train::update_online(&entry, config.max_nodes, config.max_nodes.saturating_mul(4));
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
        telemetry::record_snapshot(
            &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
            &snapshot,
        );
        let _ = std::fs::create_dir_all(Path::new(TEMPLATE_ROOT));
        telemetry::record_snapshot(
            &Path::new(TEMPLATE_ROOT).join(format!("metrics_{}.json", template_hash)),
            &snapshot,
        );
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

fn validate_planner_update(
    graph: &dag::TaskGraph,
    update: &PlannerUpdate,
    planner_max_new_nodes: usize,
    planner_max_new_edges: usize,
    failure_store: &mut FailureStore,
    iteration: u64,
    failure_constraint_threshold: usize,
    max_constraints: usize,
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
    let constraints = failure_store.constraints(failure_constraint_threshold, max_constraints);
    if !constraints.is_empty() {
        let signals = compute_graph_signals(&test_graph);
        for c in constraints {
            if let Some(err) = check_constraint(&test_graph, &signals, &c) {
                return Err(anyhow::anyhow!(err));
            }
        }
    }
    Ok(())
}

fn check_constraint(
    graph: &dag::TaskGraph,
    signals: &super::graph_algo::GraphSignals,
    constraint: &super::failure_store::Constraint,
) -> Option<String> {
    use super::failure_store::ConstraintRule;
    match &constraint.rule {
        ConstraintRule::NoCycle => signals.has_cycle.then(|| "constraint violated: NoCycle".to_string()),
        ConstraintRule::NoUnreachable => (!signals.unreachable.is_empty())
            .then(|| "constraint violated: NoUnreachable".to_string()),
        ConstraintRule::CapabilityConflict => None,
        ConstraintRule::InvalidDependency => None,
        ConstraintRule::PatternRewrite { pattern, .. } => {
            let bad = graph
                .nodes
                .iter()
                .any(|n| n.description.to_lowercase().contains(pattern));
            bad.then(|| format!("constraint violated: PatternRewrite({})", pattern))
        }
        ConstraintRule::SignatureBan => {
            let sig = graph_signature(graph);
            (sig == constraint.signature).then(|| "constraint violated: SignatureBan".to_string())
        }
    }
}

fn auto_repair_planner_update(graph: &dag::TaskGraph, update: &mut PlannerUpdate) -> u64 {
    let mut used: std::collections::HashSet<String> =
        graph.nodes.iter().map(|n| n.id.clone()).collect();
    for spec in &update.new_nodes {
        used.insert(spec.id.clone());
    }
    for spec in &update.rewrite_nodes {
        used.insert(spec.id.clone());
    }

    let mut repairs = 0u64;
    let mut repaired_nodes: Vec<decompose::TaskSpec> = Vec::new();

    for spec in update.new_nodes.drain(..) {
        let (observe, verify, mutate) = split_caps(&spec.required_capabilities);
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            let verify_id = unique_id(format!("{}_verify", spec.id), &mut used);
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            let verify_node = decompose::TaskSpec {
                id: verify_id.clone(),
                description: format!("Verify preconditions for {}: {}", spec.id, spec.description),
                deps: spec.deps.clone(),
                required_capabilities: verify_caps,
                node_type: decompose::NodeType::Analysis,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: Some("AUTO_REPAIR: split verify/mutate".to_string()),
            };
            let mut mutate_deps = spec.deps.clone();
            if !mutate_deps.contains(&verify_id) {
                mutate_deps.push(verify_id.clone());
            }
            let mutate_node = decompose::TaskSpec {
                id: spec.id,
                description: spec.description,
                deps: mutate_deps,
                required_capabilities: mutate,
                node_type: spec.node_type,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: spec.reasoning_trace,
            };
            let mutate_id = mutate_node.id.clone();
            repaired_nodes.push(verify_node);
            repaired_nodes.push(mutate_node);
            update.new_edges.push(EdgeSpec { from: verify_id, to: mutate_id });
        } else {
            let mut caps = if !mutate.is_empty() { mutate } else { verify };
            if !caps.is_empty() {
                caps.extend(observe);
                let mut spec = spec;
                spec.required_capabilities = caps;
                repaired_nodes.push(spec);
            } else {
                repaired_nodes.push(spec);
            }
        }
    }

    for spec in update.rewrite_nodes.iter_mut() {
        let (observe, verify, mutate) = split_caps(&spec.new_capabilities);
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            let verify_id = unique_id(format!("{}_verify", spec.id), &mut used);
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            update.new_nodes.push(decompose::TaskSpec {
                id: verify_id.clone(),
                description: format!("Verify preconditions for {}: {}", spec.id, spec.new_description),
                deps: Vec::new(),
                required_capabilities: verify_caps,
                node_type: decompose::NodeType::Analysis,
                priority: 5,
                budget: None,
                reasoning_trace: Some("AUTO_REPAIR: split rewrite verify/mutate".to_string()),
            });
            update.new_edges.push(EdgeSpec { from: verify_id, to: spec.id.clone() });
            spec.new_capabilities = mutate;
        } else if !mutate.is_empty() {
            let mut caps = mutate;
            caps.extend(observe);
            spec.new_capabilities = caps;
        } else {
            let mut caps = verify;
            caps.extend(observe);
            spec.new_capabilities = caps;
        }
    }

    update.new_nodes.extend(repaired_nodes);
    repairs
}

fn split_caps(caps: &[Capability]) -> (Vec<Capability>, Vec<Capability>, Vec<Capability>) {
    let mut observe = Vec::new();
    let mut verify = Vec::new();
    let mut mutate = Vec::new();
    for &cap in caps {
        match cap.class() {
            CapabilityClass::Observe => observe.push(cap),
            CapabilityClass::Verify => verify.push(cap),
            CapabilityClass::Mutate => mutate.push(cap),
        }
    }
    (observe, verify, mutate)
}

fn unique_id(base: String, used: &mut std::collections::HashSet<String>) -> String {
    if !used.contains(&base) {
        used.insert(base.clone());
        return base;
    }
    let mut idx = 1u32;
    loop {
        let candidate = format!("{}_{}", base, idx);
        if !used.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
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
                repair_attempts: 0,
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
