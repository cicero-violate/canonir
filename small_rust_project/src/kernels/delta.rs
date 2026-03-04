use crate::dag::{NodeId, Status};
use std::collections::HashMap;

/// Pure dependency propagation kernel.
///
/// Computes downstream status impacts when a node changes state.
pub fn compute_dependency_deltas(
    changed_node: &NodeId,
    new_status: Status,
    reverse_edges: &HashMap<NodeId, Vec<NodeId>>,
) -> Vec<(NodeId, Status)> {
    let mut deltas = Vec::new();

    if let Some(children) = reverse_edges.get(changed_node) {
        for child in children {
            if new_status == Status::Failed {
                deltas.push((child.clone(), Status::Blocked));
            }
        }
    }

    deltas
}
