use super::task_graph::TaskGraph;
use super::goal::GoalSpec;
use super::graph_algo;
use super::goal_embedding;
use super::objectives;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PlannerTelemetry {
    pub planner_calls: u64,
    pub planner_retries: u64,
    pub planner_failures: u64,
    pub nodes_added: u64,
    pub edges_added: u64,
    pub iterations: u64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ExecutionTelemetry {
    pub nodes_executed: u64,
    pub nodes_failed: u64,
    pub avg_latency_ms: u64,
    pub last_repair_attempts: u64,
    pub last_repair_successes: u64,
    pub last_repair_kind: Option<String>,
    pub last_snapshot_written: bool,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeQueueTelemetry {
    pub queue_depth: u64,
    pub retry_rate: f64,
    pub progress_fraction: f64,
    pub iteration_time_ms: u64,
    pub branching_factor: f64,
    pub blocked_fraction: f64,
    pub completion_velocity: f64,
    pub deadlock_rate: f64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimePolicyTelemetry {
    pub policy_prediction: f64,
    pub policy_error: f64,
    pub policy_weight_norm: f64,
    pub dataset_size: u64,
    pub policy_run_planner: bool,
    pub policy_expansion_scale: f64,
    pub policy_execution_preference: f64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeTemplateTelemetry {
    pub template_reuse: bool,
    pub template_score: f64,
    pub template_selected: Option<String>,
    pub template_mutations: u64,
    pub template_new_nodes: u64,
    pub template_new_edges: u64,
    pub template_rewrites: u64,
    pub template_mutations_reuse: u64,
    pub template_mutations_mutate: u64,
    pub template_mutations_patch: u64,
    pub template_mutations_execute: u64,
    pub mutation_success_rate: f64,
    pub mutation_reward_delta: f64,
    pub template_reuse_by_embedding: bool,
    pub embedding_cache_hits: u64,
    pub objective_delta: f64,
    pub template_hit_rate: f64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeRepairTelemetry {
    pub repair_attempts: u64,
    pub repair_success_rate: f64,
    pub repair_type: Option<String>,
    pub constraint_rejections: u64,
    pub constraint_hit_rate: f64,
    pub constraint_types: Option<String>,
    pub planner_entropy: f64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimePerformanceTelemetry {
    pub avg_capability_latency: f64,
    pub avg_capability_failure: f64,
    pub avg_node_utility: f64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeSnapshotTelemetry {
    pub snapshot_written: bool,
    pub snapshot_loaded: bool,
    pub resume_iteration: u64,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeGoalTelemetry {
    pub goal_similarity_score: f64,
    pub goal_drift: f64,
    pub planner_refocus: bool,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeTelemetry {
    #[serde(default, flatten)]
    pub queue: RuntimeQueueTelemetry,
    #[serde(default, flatten)]
    pub policy: RuntimePolicyTelemetry,
    #[serde(default, flatten)]
    pub template: RuntimeTemplateTelemetry,
    #[serde(default, flatten)]
    pub repair: RuntimeRepairTelemetry,
    #[serde(default, flatten)]
    pub performance: RuntimePerformanceTelemetry,
    #[serde(default, flatten)]
    pub snapshot: RuntimeSnapshotTelemetry,
    #[serde(default, flatten)]
    pub goal: RuntimeGoalTelemetry,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct TelemetryFrame {
    pub planner: PlannerTelemetry,
    pub exec: ExecutionTelemetry,
    pub runtime: RuntimeTelemetry,
    pub reward: f64,
    pub template_hash: Option<String>,
    pub goal: Option<String>,
}
pub fn telemetry_record_snapshot(path: &Path, snapshot: &TelemetryFrame) {
    if let Ok(pretty) = serde_json::to_string_pretty(snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}
pub fn telemetry_record_all_snapshots(snapshot: &TelemetryFrame, log_root: &str, template_root: &str, template_hash: &str) {
    telemetry_record_snapshot(&Path::new(log_root).join("planner_logs/metrics.json"), snapshot);
    telemetry_record_snapshot(&Path::new(log_root).join("metrics.json"), snapshot);
    telemetry_record_snapshot(&Path::new("/workspace/ai_sandbox/canon/state/projections/metrics.json"), snapshot);
    let _ = std::fs::create_dir_all(Path::new(template_root));
    telemetry_record_snapshot(&Path::new(template_root).join(format!("metrics_{}.json", template_hash)), snapshot);
}
pub fn telemetry_update_avg_u64(current: u64, next: u64) -> u64 {
    current.checked_add(next).map(|s| s / 2).unwrap_or(next)
}
pub fn telemetry_progress_fraction(graph: &TaskGraph) -> f64 {
    if graph.nodes.is_empty() {
        return 0.0;
    }
    let completed = graph.nodes.iter().filter(|n| n.status == super::task_graph::NodeStatus::Completed).count();
    completed as f64 / graph.nodes.len() as f64
}
pub fn telemetry_compute_reward(graph: &TaskGraph, iterations_used: u64, max_iterations: u64, goal: &GoalSpec) -> f64 {
    let n_total = graph.nodes.len() as f64;
    if n_total == 0.0 {
        return 0.0;
    }
    let n_completed = graph.nodes.iter().filter(|n| n.status == super::task_graph::NodeStatus::Completed).count() as f64;
    let n_failed = graph.nodes.iter().filter(|n| n.status == super::task_graph::NodeStatus::Failed).count() as f64;
    let iter_ratio = iterations_used as f64 / max_iterations.max(1) as f64;
    let mut reward = (n_completed / n_total) - 0.2 * iter_ratio - 0.3 * (n_failed / n_total);
    let goal_sim = telemetry_goal_similarity(graph, goal);
    reward += goal_sim * 0.3;
    const OBJECTIVE_ALPHA: f64 = 0.1;
    let objective_delta = objectives::objective_reward_delta();
    reward + (OBJECTIVE_ALPHA * objective_delta)
}

pub fn telemetry_goal_similarity(graph: &TaskGraph, goal: &GoalSpec) -> f64 {
    let graph_embed = graph_algo::graph_embedding(graph, goal.embedding.len());
    goal_embedding::goal_embedding_cosine_similarity(&goal.embedding, &graph_embed)
}
pub static PENDING_REQUESTS: AtomicU64 = AtomicU64::new(0);
pub static RESUME_ITERATION: AtomicU64 = AtomicU64::new(0);
pub fn telemetry_pending_requests() -> u64 {
    PENDING_REQUESTS.load(Ordering::Relaxed)
}
pub fn telemetry_set_resume_iteration(iter: u64) {
    RESUME_ITERATION.store(iter, Ordering::Relaxed);
}
pub fn telemetry_resume_iteration() -> u64 {
    RESUME_ITERATION.load(Ordering::Relaxed)
}
pub fn telemetry_inc_pending() {
    PENDING_REQUESTS.fetch_add(1, Ordering::Relaxed);
}
pub fn telemetry_dec_pending() {
    PENDING_REQUESTS.fetch_sub(1, Ordering::Relaxed);
}
