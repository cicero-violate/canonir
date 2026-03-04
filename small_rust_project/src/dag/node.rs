use super::Status;

pub type NodeId = String;

#[derive(Debug, Clone)]
pub struct TaskNode {
    pub id: NodeId,
    pub description: String,
    pub deps: Vec<NodeId>,
    pub status: Status,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NodeResult {
    pub node_id: NodeId,
    pub status: Status,
    pub output: Option<String>,
    pub error: Option<String>,
}
