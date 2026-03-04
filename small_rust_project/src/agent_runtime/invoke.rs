use crate::dag::{TaskNode, NodeResult};
use crate::agent_runtime::context::ExecutionContext;

/// Stateless node invocation kernel.
/// The runtime provides all state via ExecutionContext.
pub fn invoke_stateless_node(node: &TaskNode, ctx: &ExecutionContext) -> NodeResult {
    // Placeholder execution model
    // Real implementation would call an LLM or deterministic kernel

    NodeResult {
        node_id: node.id.clone(),
        status: crate::dag::Status::Completed,
        output: Some(format!("executed: {}", node.description)),
        error: None,
    }
}
