use super::capability_cost::{capability_cost_apply_node_cost_update, CapabilityCostCapabilityCostTable};
use super::config;
use super::dag;
use super::engine;
use super::telemetry::ExecutionTelemetry;
use super::LOG_ROOT;
use crate::tlog;
use anyhow::Result;
use std::path::Path;
#[derive(Default)]
pub struct RepairAttemptStats {
    pub attempts: u64,
    pub successes: u64,
    pub last_kind: Option<String>,
}
pub fn apply_node_result(
    item: Result<(String, Result<engine::ModuleNodeCallResult>, std::time::Duration)>, graph: &mut dag::ExecutionGraph, cwd: &[std::path::PathBuf], max_output_lines: usize, iter: u64,
    policy: &config::CapabilityConfigCapabilityPolicy, exec_metrics: &mut ExecutionTelemetry, repair_stats: &mut RepairAttemptStats, repair_radius: usize, max_repairs: u32,
    cost_table: &mut CapabilityCostCapabilityCostTable, cost_decay_rate: f64, cost_latency_weight: f64, cost_failure_weight: f64,
) -> Option<u128> {
    let (node_id, call_result, elapsed) = match item {
        Ok(t) => t,
        Err(e) => {
            eprintln!(r#"[capability] {{"iter":{},"event":"join_error","error":"{}"}}"#, iter, e);
            return None;
        }
    };
    let ms = elapsed.as_millis();
    let report = match engine::module_process_call_result(node_id.clone(), call_result, graph, cwd, max_output_lines, Path::new(LOG_ROOT), iter, policy, repair_radius, max_repairs) {
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
        let success = matches!(n.status, dag::NodeStatus::Completed);
        let latency_ms = ms as f64;
        let _node_cost = capability_cost_apply_node_cost_update(cost_table, n, latency_ms, success, cost_decay_rate, cost_latency_weight, cost_failure_weight);
    }
    let status_value = graph
        .get_node(&node_id)
        .and_then(|n| serde_json::to_value(n.status).ok())
        .unwrap_or_else(|| serde_json::json!(null));
    tlog::emit(
        "node_executed",
        serde_json::json!({
            "node": node_id,
            "iter": iter,
            "elapsed_ms": ms,
            "had_error": report.had_error,
            "repair_kind": report.repair_kind,
            "repair_succeeded": report.repair_succeeded,
            "status": status_value,
        }),
    );
    if report.had_error && report.repair_kind.is_some() {
        tlog::emit(
            "repair_triggered",
            serde_json::json!({
                "node": node_id,
                "iter": iter,
                "repair_kind": report.repair_kind,
                "repair_succeeded": report.repair_succeeded,
            }),
        );
    }
    Some(ms)
}
