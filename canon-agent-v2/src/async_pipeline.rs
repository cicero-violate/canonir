use crate::agent_runtime::{AgentTask, TaskQueue};
use crate::capability::capability_model_dominant_class;
use crate::capability_cost::CapabilityCostCapabilityCostTable;
use crate::config::{CapabilityConfig, CapabilityConfigCapabilityPolicy};
use crate::console;
use crate::dag::{self, ExecutionGraph, NodeStatus};
use crate::dispatch;
use crate::engine::TabManagerHandle;
use crate::execution_result::{self, RepairAttemptStats};
use crate::failure_store::FailureStore;
use crate::gpu_scheduler::driver::GpuScheduler;
use crate::graph_algo;
use crate::graph_maintenance::{self, GraphRepairMaintenanceCtx};
use crate::graph_runtime;
use crate::goal::GoalSpec;
use crate::telemetry;
use crate::ws_server::WsBridge;
use anyhow::Result;
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{interval, Duration};

pub type PlannerGenerateFn =
    Arc<dyn Fn() -> BoxFuture<'static, Result<ExecutionGraph>> + Send + Sync>;
pub type PlannerTaskFn =
    Arc<dyn Fn(Arc<Mutex<ExecutionGraph>>, u64) -> BoxFuture<'static, Result<f64>> + Send + Sync>;
pub type TemplateRewardHook =
    Arc<dyn Fn(f64) -> BoxFuture<'static, Result<()>> + Send + Sync>;
pub type TemplateFailureHook =
    Arc<dyn Fn(&str) -> BoxFuture<'static, Result<()>> + Send + Sync>;
pub type TemplateTelemetryHook =
    Arc<dyn Fn() -> BoxFuture<'static, Result<(Option<String>, telemetry::RuntimeTemplateTelemetry)>> + Send + Sync>;

pub async fn run_async_pipeline(
    graph: ExecutionGraph,
    planner_generate: PlannerGenerateFn,
    planner_task: Option<PlannerTaskFn>,
    template_reward: Option<TemplateRewardHook>,
    template_failure: Option<TemplateFailureHook>,
    template_telemetry: Option<TemplateTelemetryHook>,
    bridge: WsBridge,
    config: Arc<CapabilityConfig>,
    role_rr: Arc<tokio::sync::Mutex<HashMap<String, usize>>>,
    tabs: TabManagerHandle,
    cwd: &[PathBuf],
    _workspace_listing: &str,
    endpoint: crate::config::CapabilityConfigLlmEndpoint,
    exec_role: &str,
    policy: Arc<CapabilityConfigCapabilityPolicy>,
    context_radius: usize,
    max_concurrency: usize,
    max_iterations: u64,
    tab_cooldown_ms: u64,
    retry_count: u32,
    retry_delay: u64,
    max_output_lines: usize,
    goal_spec: &GoalSpec,
    log_root: &Path,
) -> Result<f64> {
    let (queue, mut rx) = TaskQueue::new();
    let graph = Arc::new(Mutex::new(graph));
    let inflight = Arc::new(Mutex::new(HashSet::new()));
    let exec_metrics = Arc::new(Mutex::new(telemetry::ExecutionTelemetry::default()));
    let repair_stats = Arc::new(Mutex::new(RepairAttemptStats::default()));
    let cost_table = Arc::new(Mutex::new(CapabilityCostCapabilityCostTable::snapshot_store_load()));
    let failure_store = Arc::new(Mutex::new(FailureStore::snapshot_store_load(
        "async_pipeline",
    )));
    let semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
    let iter = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let watchdog_graph = graph.clone();
    let watchdog_inflight = inflight.clone();
    let watchdog_stop = stop.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            if watchdog_stop.load(Ordering::Relaxed) {
                break;
            }
            let inflight_ids = watchdog_inflight
                .lock()
                .await
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            if inflight_ids.is_empty() {
                continue;
            }
            let g = watchdog_graph.lock().await;
            let mut counts = [0usize; 6];
            for n in &g.nodes {
                counts[n.status as usize] += 1;
            }
            let inflight_statuses = inflight_ids
                .iter()
                .filter_map(|id| {
                    g.nodes
                        .iter()
                        .find(|n| n.id == *id)
                        .map(|n| format!("{}={:?}", id, n.status))
                })
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "[async] watchdog inflight={} status_counts(pending={},ready={},running={},completed={},failed={},blocked={}) ids=[{}]",
                inflight_ids.len(),
                counts[dag::NodeStatus::Pending as usize],
                counts[dag::NodeStatus::Ready as usize],
                counts[dag::NodeStatus::Running as usize],
                counts[dag::NodeStatus::Completed as usize],
                counts[dag::NodeStatus::Failed as usize],
                counts[dag::NodeStatus::Blocked as usize],
                inflight_statuses
            );
        }
    });

    let scheduler_queue = queue.clone();
    let scheduler_graph = graph.clone();
    let scheduler_inflight = inflight.clone();
    let scheduler_iter = iter.clone();
    let scheduler_stop = stop.clone();
    let scheduler_goal = goal_spec.clone();
    let scheduler_log_root = log_root.to_path_buf();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(200));
        let mut last_plan_iter = 0u64;
        let mut last_ready_iter = 0u64;
        let result = std::panic::AssertUnwindSafe(async {
            loop {
                ticker.tick().await;
                if scheduler_stop.load(Ordering::Relaxed) {
                    break;
                }
                let tick = scheduler_iter.fetch_add(1, Ordering::Relaxed) + 1;
                if tick > max_iterations {
                    scheduler_stop.store(true, Ordering::Relaxed);
                    scheduler_queue.enqueue(AgentTask::Shutdown);
                    break;
                }
                let mut g = scheduler_graph.lock().await;
                dag::task_graph_resolve_ready(&mut g);
                let mut ready = GpuScheduler::schedule(&g);
                if ready.is_empty() {
                    // Fallback: GPU scheduler can return empty if CUDA path misbehaves.
                    ready = g
                        .ready_nodes()
                        .iter()
                        .map(|n| n.id.clone())
                        .collect();
                }
                let running = g
                    .nodes
                    .iter()
                    .filter(|n| n.status == NodeStatus::Running)
                    .count();
                if g.all_completed() || g.has_failed() {
                    scheduler_stop.store(true, Ordering::Relaxed);
                    scheduler_queue.enqueue(AgentTask::Shutdown);
                    break;
                }
                if ready.is_empty() {
                    if tick.saturating_sub(last_ready_iter) >= 2 && running == 0 {
                        if GpuScheduler::detect_deadlock(&g) {
                            scheduler_queue.enqueue(AgentTask::Plan);
                        } else {
                            scheduler_queue.enqueue(AgentTask::Plan);
                        }
                        last_plan_iter = tick;
                    }
                    scheduler_queue.enqueue(AgentTask::MaintainGraph);
                } else {
                    drop(g);
                    let mut inflight_guard = scheduler_inflight.lock().await;
                    for id in ready {
                        if inflight_guard.insert(id.clone()) {
                            scheduler_queue.enqueue(AgentTask::ExecuteNode(id));
                        }
                    }
                    last_ready_iter = tick;
                }
                if scheduler_goal.raw.is_empty() {
                    // no-op; ensures goal is used to keep closure alive
                }
                if scheduler_log_root.as_os_str().is_empty() {
                    // no-op
                }
            }
        })
        .catch_unwind()
        .await;
        if result.is_err() {
            eprintln!("[async] scheduler task panicked; forcing shutdown");
            scheduler_stop.store(true, Ordering::Relaxed);
            scheduler_queue.enqueue(AgentTask::Shutdown);
        }
    });

    // Dispatcher loop
    loop {
        let task = tokio::select! {
            task = rx.recv() => task,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                // If we're idle with nothing inflight, check graph terminal condition.
                let inflight_count = inflight.lock().await.len();
                if inflight_count == 0 {
                    let g = graph.lock().await;
                    if g.all_completed() || g.has_failed() {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                continue;
            }
        };
        let Some(task) = task else {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            continue;
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match task {
            AgentTask::Shutdown => {
                stop.store(true, Ordering::Relaxed);
                break;
            }
            AgentTask::Plan => {
                eprintln!("[async] planner task");
                if let Some(task_fn) = planner_task.as_ref() {
                    let _ = task_fn(graph.clone(), iter.load(Ordering::Relaxed)).await?;
                } else {
                    let mut g = graph.lock().await;
                    let new_graph = planner_generate().await?;
                    *g = new_graph;
                    graph_runtime::ensure_render_reachable(&mut g);
                    graph_runtime::must_validate_graph_semantics(&g, Some(goal_spec))?;
                    g.rebuild_index();
                }
                inflight.lock().await.clear();
            }
            AgentTask::MaintainGraph => {
                let mut g = graph.lock().await;
                if g.nodes.is_empty() {
                    drop(g);
                    queue.enqueue(AgentTask::Plan);
                    continue;
                }
                let features = graph_algo::compute_graph_features_parallel(&g);
                let ctx = GraphRepairMaintenanceCtx {
                    graph: &mut g,
                    log_dir: log_root,
                    iter: iter.load(Ordering::Relaxed),
                    goal: Some(goal_spec),
                    features_retry_rate: features.retry_rate,
                    features_failed_fraction: features.failed_fraction,
                    features_branching_factor: features.branching_factor,
                    prune_unlinked: config.prune_unlinked,
                    auto_prune: config.auto_prune,
                    prune_min_age: config.prune_min_age,
                    prune_threshold: config.prune_threshold,
                    recovery_retry_rate_threshold: config.recovery_retry_rate_threshold,
                    recovery_failed_fraction_threshold: config.recovery_failed_fraction_threshold,
                };
                let _ = graph_maintenance::repair_graph(ctx);
                g.rebuild_index();
                let goal_sim = telemetry::telemetry_goal_similarity(&g, goal_spec);
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
                let mut template_hash = None;
                let mut template_rt = telemetry::RuntimeTemplateTelemetry::default();
                if let Some(hook) = template_telemetry.as_ref() {
                    if let Ok((hash, rt)) = hook().await {
                        template_hash = hash;
                        template_rt = rt;
                    }
                }
                let mut runtime = runtime;
                runtime.template = template_rt;
                let snapshot = telemetry::TelemetryFrame {
                    planner: Default::default(),
                    exec: exec_metrics.lock().await.clone(),
                    runtime,
                    reward: 0.0,
                    template_hash,
                    goal: Some(goal_spec.raw.clone()),
                };
                telemetry::telemetry_record_snapshot(&log_root.join("metrics.json"), &snapshot);
                telemetry::telemetry_record_snapshot(
                    &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                    &snapshot,
                );
            }
            AgentTask::RepairGraph => {
                let mut g = graph.lock().await;
                let features = graph_algo::compute_graph_features_parallel(&g);
                let ctx = GraphRepairMaintenanceCtx {
                    graph: &mut g,
                    log_dir: log_root,
                    iter: iter.load(Ordering::Relaxed),
                    goal: Some(goal_spec),
                    features_retry_rate: features.retry_rate,
                    features_failed_fraction: features.failed_fraction,
                    features_branching_factor: features.branching_factor,
                    prune_unlinked: config.prune_unlinked,
                    auto_prune: config.auto_prune,
                    prune_min_age: config.prune_min_age,
                    prune_threshold: config.prune_threshold,
                    recovery_retry_rate_threshold: config.recovery_retry_rate_threshold,
                    recovery_failed_fraction_threshold: config.recovery_failed_fraction_threshold,
                };
                let _ = graph_maintenance::repair_graph(ctx);
                g.rebuild_index();
            }
            AgentTask::ExecuteNode(node_id) => {
                let graph_clone = graph.clone();
                let inflight_clone = inflight.clone();
                let queue_clone = queue.clone();
                let exec_metrics = exec_metrics.clone();
                let repair_stats = repair_stats.clone();
                let cost_table = cost_table.clone();
                let failure_store = failure_store.clone();
                let sem = semaphore.clone();
                let node_id_clone = node_id.clone();
                let bridge = bridge.clone();
                let tabs = tabs.clone();
                let role_rr = role_rr.clone();
                let cwd = cwd.to_vec();
                let endpoint = endpoint.clone();
                let policy = policy.clone();
                let config = config.clone();
                let goal_spec = goal_spec.clone();
                let log_root = log_root.to_path_buf();
                let exec_role = exec_role.to_string();
                let iter = iter.clone();
                let template_failure = template_failure.clone();
                let template_telemetry = template_telemetry.clone();
                tokio::spawn(async move {
                    let result = std::panic::AssertUnwindSafe(async {
                        let mut g = graph_clone.lock().await;
                        let node = match g.get_node(&node_id_clone).cloned() {
                            Some(n) => n,
                            None => {
                                return;
                            }
                        };
                        let auth = match dag::task_graph_grant_authority(&node) {
                            Ok(a) => a,
                            Err(e) => {
                                eprintln!(
                                    r#"[capability] {{"event":"authority_error","node":"{}","error":"{}"}}"#,
                                    node.id, e
                                );
                                let _ = g.update_status(&node.id, NodeStatus::Ready);
                                return;
                            }
                        };
                        let mode = capability_model_dominant_class(&node.required_capabilities);
                        let mode_label = console::console_mode_tag(mode);
                        let ctx = dispatch::node_dispatch_resolve_endpoint(
                            config.as_ref(),
                            &role_rr,
                            &exec_role,
                            (
                                &endpoint.id,
                                &endpoint.url,
                                endpoint.max_tabs,
                                endpoint.stateful,
                                &endpoint.role_markdown,
                            ),
                            cwd[0].clone(),
                            log_root.clone(),
                        )
                        .await;
                        dispatch::log_node_dispatch(&node, &mode_label, &ctx.endpoint_id);
                        let context = graph_runtime::collect_execution_context(
                            &mut g,
                            &node.id,
                            context_radius,
                        );
                        drop(g);
                        let fut = dispatch::dispatch_node_call(
                            node,
                            auth,
                            &bridge,
                            &tabs,
                            sem,
                            ctx,
                            context,
                            iter.load(Ordering::Relaxed),
                            retry_count,
                            retry_delay,
                            tab_cooldown_ms,
                        );
                        let item = fut.await;
                        let mut g = graph_clone.lock().await;
                        let mut metrics = exec_metrics.lock().await;
                        let mut repair = repair_stats.lock().await;
                        let mut cost = cost_table.lock().await;
                        eprintln!(
                            "[async] apply_node_result start node={}",
                            node_id_clone
                        );
                        if let Some(ms) = execution_result::apply_node_result(
                            item,
                            &mut g,
                            &cwd,
                            max_output_lines,
                            iter.load(Ordering::Relaxed),
                            policy.as_ref(),
                            &mut metrics,
                            &mut repair,
                            config.repair_radius,
                            config.max_repairs_per_node,
                            &mut cost,
                            config.cost_decay_rate,
                            config.cost_latency_weight,
                            config.cost_failure_weight,
                        ) {
                            let _ = ms;
                        }
                        eprintln!(
                            "[async] apply_node_result done node={}",
                            node_id_clone
                        );
                        if g.has_failed() {
                            let mut fs = failure_store.lock().await;
                            fs.record_graph("exec_failure", &g, iter.load(Ordering::Relaxed));
                            if let Some(hook) = template_failure.as_ref() {
                                let _ = hook("exec_failure").await;
                            }
                        }
                        if graph_runtime::ensure_render_reachable(&mut g) {
                            queue_clone.enqueue(AgentTask::RepairGraph);
                        }
                        if graph_runtime::must_validate_graph_semantics(&g, Some(&goal_spec)).is_err() {
                            queue_clone.enqueue(AgentTask::RepairGraph);
                        }
                        let features = graph_algo::compute_graph_features_parallel(&g);
                        let goal_sim = telemetry::telemetry_goal_similarity(&g, &goal_spec);
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
                        let mut template_hash = None;
                        let mut template_rt = telemetry::RuntimeTemplateTelemetry::default();
                        if let Some(hook) = template_telemetry.as_ref() {
                            if let Ok((hash, rt)) = hook().await {
                                template_hash = hash;
                                template_rt = rt;
                            }
                        }
                        runtime.template = template_rt;
                        let snapshot = telemetry::TelemetryFrame {
                            planner: Default::default(),
                            exec: exec_metrics.lock().await.clone(),
                            runtime,
                            reward: 0.0,
                            template_hash,
                            goal: Some(goal_spec.raw.clone()),
                        };
                        telemetry::telemetry_record_snapshot(&log_root.join("metrics.json"), &snapshot);
                        telemetry::telemetry_record_snapshot(
                            &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
                            &snapshot,
                        );
                    })
                    .catch_unwind()
                    .await;
                    if result.is_err() {
                        eprintln!(
                            r#"[async] node task panicked; node={} (forcing inflight clear)"#,
                            node_id_clone
                        );
                    }
                    inflight_clone.lock().await.remove(&node_id_clone);
                });
            }
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }

    let g = graph.lock().await;
    let reward = telemetry::telemetry_compute_reward(
        &g,
        iter.load(Ordering::Relaxed),
        max_iterations,
        goal_spec,
    );
    if let Some(hook) = template_reward.as_ref() {
        hook(reward).await?;
    }
    let features = graph_algo::compute_graph_features_parallel(&g);
    let goal_sim = telemetry::telemetry_goal_similarity(&g, goal_spec);
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
    let mut template_hash = None;
    let mut template_rt = telemetry::RuntimeTemplateTelemetry::default();
    if let Some(hook) = template_telemetry.as_ref() {
        if let Ok((hash, rt)) = hook().await {
            template_hash = hash;
            template_rt = rt;
        }
    }
    runtime.template = template_rt;
    let snapshot = telemetry::TelemetryFrame {
        planner: Default::default(),
        exec: exec_metrics.lock().await.clone(),
        runtime,
        reward,
        template_hash,
        goal: Some(goal_spec.raw.clone()),
    };
    telemetry::telemetry_record_snapshot(&log_root.join("metrics.json"), &snapshot);
    telemetry::telemetry_record_snapshot(
        &Path::new("/workspace/ai_sandbox/canon/agent_logs/metrics.json"),
        &snapshot,
    );
    Ok(reward)
}
