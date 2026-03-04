use crate::dag::{TaskGraph, NodeId};
use crate::scheduler::build_frontier;
use crate::agent_runtime::invoke::invoke_stateless_node;
use crate::agent_runtime::context::ExecutionContext;

pub struct AgentRuntime {
    pub graph: TaskGraph,
}

impl AgentRuntime {
    pub fn new(graph: TaskGraph) -> Self {
        Self { graph }
    }

    pub fn step(&mut self, ctx: &ExecutionContext) {
        let mut frontier = build_frontier(&self.graph);

        while let Some(node_id) = frontier.pop() {
            if let Some(node) = self.graph.get_node(&node_id).cloned() {
                let result = invoke_stateless_node(&node, ctx);
                self.graph.apply_result(node_id, result);
            }
        }
    }
}
