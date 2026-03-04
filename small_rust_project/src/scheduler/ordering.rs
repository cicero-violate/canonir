use crate::dag::TaskNode;
use crate::dag::NodeId;

/// Deterministic ordering of ready nodes.
///
/// Current strategy:
/// 1. Stable lexicographic ordering by NodeId
/// 2. Deterministic across executions
pub fn deterministic_ready_order(nodes: Vec<&TaskNode>) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = nodes.into_iter().map(|n| n.id.clone()).collect();
    ids.sort();
    ids
}
