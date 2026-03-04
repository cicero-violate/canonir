use std::collections::HashMap;

use super::{NodeId, Status, TaskNode, NodeResult};

#[derive(Debug, Clone)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn new(nodes: Vec<TaskNode>) -> Self {
        Self { nodes }
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    pub fn get_node_mut(&mut self, id: &NodeId) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| &n.id == id)
    }

    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        self.nodes
            .iter()
            .filter(|n| n.status == Status::Ready)
            .collect()
    }

    pub fn apply_result(&mut self, node_id: NodeId, result: NodeResult) {
        if let Some(node) = self.get_node_mut(&node_id) {
            node.status = result.status;
            node.result = result.output;
            node.error = result.error;
        }
    }

    pub fn dependency_statuses(&self, node: &TaskNode) -> Vec<Status> {
        node.deps
            .iter()
            .filter_map(|d| self.get_node(d))
            .map(|n| n.status)
            .collect()
    }

    pub fn reverse_edges(&self) -> HashMap<NodeId, Vec<NodeId>> {
        let mut map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for node in &self.nodes {
            for dep in &node.deps {
                map.entry(dep.clone())
                    .or_default()
                    .push(node.id.clone());
            }
        }

        map
    }
}
