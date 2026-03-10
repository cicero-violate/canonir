use crate::capability_types::CapabilityMode;
use crate::dag::{ExecutionGraph, NodeStatus};
use crate::planner_state::{PlannerStage, PlannerStagePersist};
use std::path::Path;

pub fn must_terminal_lock(graph: &ExecutionGraph, node_id: &str) -> bool {
    graph.nodes.iter().find(|n| n.id == node_id).map(|n| matches!(n.status, NodeStatus::Completed | NodeStatus::Failed)).unwrap_or(false)
}

pub fn must_increment_readonly_fail_on_parse(
    graph: &mut ExecutionGraph,
    node_id: &str,
    max_node_retries: u32,
) -> (bool, bool) {
    if let Some(n) = graph.get_node_mut(node_id) {
        let observe_only = n
            .required_capabilities
            .iter()
            .all(|c| c.class() == CapabilityMode::Observe);
        if observe_only {
            n.readonly_fail_count += 1;
            let budget = n.budget.unwrap_or(max_node_retries);
            return (true, n.readonly_fail_count >= budget);
        }
    }
    (false, false)
}

pub fn must_fail_node_with_error(graph: &mut ExecutionGraph, node_id: &str, err: &str) {
    let _ = graph.update_status(node_id, NodeStatus::Failed);
    if let Some(n) = graph.get_node_mut(node_id) {
        n.error = Some(err.to_string());
    }
}

pub fn must_fail_node_if_repair_exhausted(
    graph: &mut ExecutionGraph,
    node_id: &str,
    max_repairs: u32,
) {
    let stale = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.repair_attempts >= max_repairs && n.error.is_some())
        .unwrap_or(false);
    if stale {
        let _ = graph.update_status(node_id, NodeStatus::Failed);
    }
}

pub fn must_render_blocked_fail_or_ready(
    graph: &mut ExecutionGraph,
    node_id: &str,
    max_node_retries: u32,
    err: &str,
) -> bool {
    let repair_exhausted = graph
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.repair_attempts >= n.budget.unwrap_or(max_node_retries))
        .unwrap_or(false);
    if repair_exhausted {
        must_fail_node_with_error(graph, node_id, err);
        true
    } else {
        let _ = graph.update_status(node_id, NodeStatus::Ready);
        false
    }
}

pub fn must_blocked_has_no_ready(graph: &ExecutionGraph) {
    let ready = graph.nodes.iter().filter(|n| n.status == NodeStatus::Ready).count();
    if ready > 0 {
        eprintln!(
            "[logs] invariant_violation blocked_with_ready ready_count={}",
            ready
        );
    }
}

pub fn must_retry_only_if_terminal_progress(prev: usize, next: usize) -> bool {
    next > prev
}

pub fn must_progress_monotonic(prev: usize, next: usize) {
    debug_assert!(next >= prev, "progress regressed");
}

pub fn must_replan_if_all_stalled(graph: &ExecutionGraph, planner_stage_path: Option<&Path>, tick: u64) -> bool {
    let all_stalled = graph.nodes.iter().all(|n| {
        matches!(
            n.status,
            NodeStatus::Completed | NodeStatus::Failed | NodeStatus::Blocked
        )
    });
    if all_stalled {
        if let Some(path) = planner_stage_path {
            PlannerStagePersist::save(path, PlannerStage::ReuseTemplate, tick);
        }
    }
    all_stalled
}

pub fn must_terminal_nonzero(graph: &ExecutionGraph) {
    #[cfg(debug_assertions)]
    {
        let terminal = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.status, NodeStatus::Completed | NodeStatus::Failed))
            .count();
        debug_assert!(terminal > 0, "I15 violated: no terminal nodes after failure");
    }
}
