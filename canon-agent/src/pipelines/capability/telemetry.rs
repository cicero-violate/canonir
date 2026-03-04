use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::dag::TaskGraph;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PlannerMetrics {
    pub planner_calls: u64,
    pub planner_retries: u64,
    pub planner_failures: u64,
    pub nodes_added: u64,
    pub edges_added: u64,
    pub iterations: u64,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ExecMetrics {
    pub nodes_executed: u64,
    pub nodes_failed: u64,
    pub avg_latency_ms: u64,
    pub last_repair_attempts: u64,
    pub last_repair_successes: u64,
    pub last_repair_kind: Option<String>,
    pub last_snapshot_written: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RuntimeMetrics {
    pub queue_depth: u64,
    pub retry_rate: f64,
    pub progress_fraction: f64,
    pub iteration_time_ms: u64,
    pub branching_factor: f64,
    pub blocked_fraction: f64,
    pub completion_velocity: f64,
    pub policy_prediction: f64,
    pub policy_error: f64,
    pub policy_weight_norm: f64,
    pub dataset_size: u64,
    pub deadlock_rate: f64,
    pub policy_run_planner: bool,
    pub policy_expansion_scale: f64,
    pub policy_execution_preference: f64,
    pub template_reuse: bool,
    pub template_score: f64,
    pub template_selected: Option<String>,
    pub repair_attempts: u64,
    pub repair_success_rate: f64,
    pub repair_type: Option<String>,
    pub constraint_rejections: u64,
    pub constraint_hit_rate: f64,
    pub constraint_types: Option<String>,
    pub avg_capability_latency: f64,
    pub avg_capability_failure: f64,
    pub avg_node_utility: f64,
    pub template_mutations: u64,
    pub mutation_success_rate: f64,
    pub mutation_reward_delta: f64,
    pub snapshot_written: bool,
    pub snapshot_loaded: bool,
    pub resume_iteration: u64,
    pub goal_similarity_score: f64,
    pub template_reuse_by_embedding: bool,
    pub embedding_cache_hits: u64,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct TelemetrySnapshot {
    pub planner: PlannerMetrics,
    pub exec: ExecMetrics,
    pub runtime: RuntimeMetrics,
    pub reward: f64,
    pub template_hash: Option<String>,
    pub goal: Option<String>,
}

pub fn record_snapshot(path: &Path, snapshot: &TelemetrySnapshot) {
    if let Ok(pretty) = serde_json::to_string_pretty(snapshot) {
        let _ = std::fs::write(path, pretty);
    }
}

pub fn progress_fraction(graph: &TaskGraph) -> f64 {
    if graph.nodes.is_empty() {
        return 0.0;
    }
    let completed = graph
        .nodes
        .iter()
        .filter(|n| n.status == super::dag::Status::Completed)
        .count();
    completed as f64 / graph.nodes.len() as f64
}

pub fn compute_reward(graph: &TaskGraph, iterations_used: u64, max_iterations: u64) -> f64 {
    let n_total = graph.nodes.len() as f64;
    if n_total == 0.0 { return 0.0; }
    let n_completed = graph.nodes.iter().filter(|n| n.status == super::dag::Status::Completed).count() as f64;
    let n_failed = graph.nodes.iter().filter(|n| n.status == super::dag::Status::Failed).count() as f64;
    let iter_ratio = iterations_used as f64 / max_iterations.max(1) as f64;
    (n_completed / n_total) - 0.2 * iter_ratio - 0.3 * (n_failed / n_total)
}

pub static PENDING_REQUESTS: AtomicU64 = AtomicU64::new(0);
pub static RESUME_ITERATION: AtomicU64 = AtomicU64::new(0);

pub fn pending_requests() -> u64 {
    PENDING_REQUESTS.load(Ordering::Relaxed)
}

pub fn set_resume_iteration(iter: u64) {
    RESUME_ITERATION.store(iter, Ordering::Relaxed);
}

pub fn resume_iteration() -> u64 {
    RESUME_ITERATION.load(Ordering::Relaxed)
}

pub fn inc_pending() {
    PENDING_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn dec_pending() {
    PENDING_REQUESTS.fetch_sub(1, Ordering::Relaxed);
}
