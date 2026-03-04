//! Deterministic DAG Runtime Skeleton
//! Implements core runtime structures for the DAG-controlled multi-agent framework.

use std::collections::{HashMap, HashSet};

pub type NodeId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct DagNode {
    pub id: NodeId,
    pub deps: Vec<NodeId>,
    pub node_type: NodeType,
    pub required_capabilities: Vec<String>,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Analysis,
    Render,
}

#[derive(Debug)]
pub struct DagGraph {
    pub nodes: HashMap<NodeId, DagNode>,
}

impl DagGraph {
    pub fn new(nodes: Vec<DagNode>) -> Self {
        let map = nodes
            .into_iter()
            .map(|n| (n.id.clone(), n))
            .collect::<HashMap<_, _>>();

        Self { nodes: map }
    }

    pub fn validate_acyclic(&self) -> bool {
        fn dfs(
            node: &NodeId,
            graph: &DagGraph,
            visiting: &mut HashSet<NodeId>,
            visited: &mut HashSet<NodeId>,
        ) -> bool {
            if visiting.contains(node) {
                return false;
            }

            if visited.contains(node) {
                return true;
            }

            visiting.insert(node.clone());

            if let Some(n) = graph.nodes.get(node) {
                for dep in &n.deps {
                    if !dfs(dep, graph, visiting, visited) {
                        return false;
                    }
                }
            }

            visiting.remove(node);
            visited.insert(node.clone());

            true
        }

        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();

        self.nodes
            .keys()
            .all(|id| dfs(id, self, &mut visiting, &mut visited))
    }
}

pub fn compute_ready_nodes(graph: &DagGraph) -> Vec<NodeId> {
    graph
        .nodes
        .values()
        .filter(|node| node.status == NodeStatus::Pending)
        .filter(|node| {
            node.deps.iter().all(|d| {
                graph
                    .nodes
                    .get(d)
                    .map(|n| n.status == NodeStatus::Completed)
                    .unwrap_or(false)
            })
        })
        .map(|n| n.id.clone())
        .collect()
}

pub fn schedule(frontier: Vec<NodeId>) -> Vec<NodeId> {
    let mut ordered = frontier;
    ordered.sort();
    ordered
}

pub fn transition_status(node: &mut DagNode, next: NodeStatus) -> Result<(), String> {
    use NodeStatus::*;

    let valid = match (&node.status, &next) {
        (Pending, Ready) => true,
        (Ready, Running) => true,
        (Running, Completed) => true,
        (Running, Failed) => true,
        (_, Skipped) => true,
        _ => false,
    };

    if valid {
        node.status = next;
        Ok(())
    } else {
        Err(format!("invalid state transition: {:?} -> {:?}", node.status, next))
    }
}
