use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlannerMetricsReport {
    pub mutation_attempts: u64,
    pub accepted_mutations: u64,
    pub utility_gain: f64,
    pub convergence_rate: f64,
    pub planner_calls: u64,
    pub planner_retries: u64,
    pub planner_failures: u64,
    pub iterations: u64,
    pub generated_at_ms: u64,
}

pub fn write_planner_metrics(path: &Path, report: &PlannerMetricsReport) {
    if let Ok(payload) = serde_json::to_string_pretty(report) {
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(path, payload);
    }
}

pub fn build_planner_metrics(
    mutation_attempts: u64,
    accepted_mutations: u64,
    utility_gain: f64,
    planner_calls: u64,
    planner_retries: u64,
    planner_failures: u64,
    iterations: u64,
) -> PlannerMetricsReport {
    let convergence_rate = if mutation_attempts == 0 {
        0.0
    } else {
        accepted_mutations as f64 / mutation_attempts as f64
    };
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    PlannerMetricsReport {
        mutation_attempts,
        accepted_mutations,
        utility_gain,
        convergence_rate,
        planner_calls,
        planner_retries,
        planner_failures,
        iterations,
        generated_at_ms,
    }
}
