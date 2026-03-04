use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::dag::TaskGraph;

#[derive(Debug, Default, Serialize, Clone)]
pub struct PlannerMetrics {
    pub planner_calls: u64,
    pub planner_retries: u64,
    pub planner_failures: u64,
    pub nodes_added: u64,
    pub edges_added: u64,
    pub iterations: u64,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct ExecMetrics {
    pub nodes_executed: u64,
    pub nodes_failed: u64,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct RuntimeMetrics {
    pub queue_depth: u64,
    pub retry_rate: f64,
    pub progress_fraction: f64,
    pub iteration_time_ms: u64,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct TelemetrySnapshot {
    pub planner: PlannerMetrics,
    pub exec: ExecMetrics,
    pub runtime: RuntimeMetrics,
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

pub static PENDING_REQUESTS: AtomicU64 = AtomicU64::new(0);

pub fn pending_requests() -> u64 {
    PENDING_REQUESTS.load(Ordering::Relaxed)
}

pub fn inc_pending() {
    PENDING_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn dec_pending() {
    PENDING_REQUESTS.fetch_sub(1, Ordering::Relaxed);
}
