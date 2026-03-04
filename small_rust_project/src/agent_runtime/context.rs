use crate::dag::{TaskGraph, NodeId};

/// ExecutionContext contains immutable runtime state
/// passed into stateless node execution.
#[derive(Clone)]
pub struct ExecutionContext {
    pub graph_snapshot: TaskGraph,
}

impl ExecutionContext {
    pub fn new(graph: &TaskGraph) -> Self {
        Self {
            graph_snapshot: graph.clone(),
        }
    }

    pub fn get_node_output(&self, node: &NodeId) -> Option<String> {
        self.graph_snapshot
            .get_node(node)
            .and_then(|n| n.result.clone())
    }
}
