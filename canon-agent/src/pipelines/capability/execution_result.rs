use std::path::Path;

use super::capability_cost::{apply_node_cost_update, CapabilityCostTable};
use super::config;
use super::dag;
use super::engine;
use super::telemetry::ExecMetrics;
use super::LOG_ROOT;
use anyhow::Result;

#[derive(Default)]
pub struct RepairStats {
    pub attempts: u64,
    pub successes: u64,
    pub last_kind: Option<String>,
}

pub fn process_node_result(
    item: Result<(String, Result<engine::NodeCallResult>, std::time::Duration)>, graph: &mut dag::TaskGraph, cwd: &[std::path::PathBuf], max_output_lines: usize, iter: u64,
    policy: &config::CapabilityPolicy, exec_metrics: &mut ExecMetrics, repair_stats: &mut RepairStats, repair_radius: usize, max_repairs: u32, cost_table: &mut CapabilityCostTable,
    cost_decay_rate: f64, cost_latency_weight: f64, cost_failure_weight: f64,
) -> Option<u128> {
    let (node_id, call_result, elapsed) = match item {
        Ok(t) => t,
        Err(e) => {
            eprintln!(r#"[capability] {{"iter":{},"event":"join_error","error":"{}"}}"#, iter, e);
            return None;
        }
    };
    let ms = elapsed.as_millis();
    let report = match engine::process_call_result(node_id.clone(), call_result, graph, cwd, max_output_lines, Path::new(LOG_ROOT), iter, policy, repair_radius, max_repairs) {
        Ok(report) => report,
        Err(e) => {
            eprintln!(r#"[capability] {{"iter":{},"event":"call_or_apply_error","node":"{}","error":"{}"}}"#, iter, node_id, e);
            return None;
        }
    };
    if report.had_error {
        repair_stats.attempts += 1;
        if report.repair_succeeded {
            repair_stats.successes += 1;
        }
        repair_stats.last_kind = report.repair_kind.clone();
        exec_metrics.nodes_failed += 1;
    }
    if let Some(n) = graph.get_node_mut(&node_id) {
        let success = matches!(n.status, dag::Status::Completed);
        let latency_ms = ms as f64;
        let _node_cost = apply_node_cost_update(cost_table, n, latency_ms, success, cost_decay_rate, cost_latency_weight, cost_failure_weight);
    }
    Some(ms)
}
