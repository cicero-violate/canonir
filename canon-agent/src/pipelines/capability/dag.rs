use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::capability::{assert_mut_verify_disjoint, Capability};
use super::decompose::NodeType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub description: String,
    pub status: Status,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<Capability>,
    #[serde(default)]
    pub node_type: NodeType,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add_node(&mut self, node: TaskNode) {
        self.nodes.push(node);
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        self.nodes.iter().filter(|n| n.status == Status::Ready).collect()
    }

    pub fn all_completed(&self) -> bool {
        !self.nodes.is_empty() && self.nodes.iter().all(|n| n.status == Status::Completed)
    }

    pub fn has_failed(&self) -> bool {
        self.nodes.iter().any(|n| n.status == Status::Failed)
    }

    pub fn update_status(&mut self, id: &str, status: Status) -> Result<(), String> {
        let node = self.get_node_mut(id).ok_or_else(|| format!("unknown node id: {id}"))?;
        if !transition_allowed(node.status, status) {
            return Err(format!("invalid status transition: {:?} -> {:?} for {}", node.status, status, id));
        }
        node.status = status;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen = HashSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id.clone()) {
                return Err(format!("duplicate node id: {}", n.id));
            }
        }
        let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        for n in &self.nodes {
            for d in &n.deps {
                if !ids.contains(d.as_str()) {
                    return Err(format!("node {} references unknown dep {}", n.id, d));
                }
            }
        }
        for n in &self.nodes {
            let caps: HashSet<Capability> = n.required_capabilities.iter().copied().collect();
            assert_mut_verify_disjoint(&caps).map_err(|e| format!("node {}: {}", n.id, e))?;
        }
        detect_cycle(self)?;
        Ok(())
    }
}

fn transition_allowed(from: Status, to: Status) -> bool {
    matches!(
        (from, to),
        (Status::Pending, Status::Ready)
            | (Status::Pending, Status::Blocked)
            | (Status::Ready, Status::Running)
            | (Status::Running, Status::Ready)
            | (Status::Running, Status::Completed)
            | (Status::Running, Status::Failed)
            | (Status::Blocked, Status::Ready)
    )
}

fn detect_cycle(graph: &TaskGraph) -> Result<(), String> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in &graph.nodes {
        adj.entry(n.id.as_str()).or_default();
        for d in &n.deps {
            adj.entry(d.as_str()).or_default().push(n.id.as_str());
        }
    }

    let mut visited = HashSet::new();
    let mut stack = HashSet::new();

    for n in &graph.nodes {
        if !visited.contains(n.id.as_str()) {
            if dfs_cycle(n.id.as_str(), &adj, &mut visited, &mut stack) {
                return Err("cycle detected in task graph".into());
            }
        }
    }
    Ok(())
}

fn dfs_cycle<'a>(node: &'a str, adj: &HashMap<&'a str, Vec<&'a str>>, visited: &mut HashSet<&'a str>, stack: &mut HashSet<&'a str>) -> bool {
    visited.insert(node);
    stack.insert(node);

    if let Some(nexts) = adj.get(node) {
        for &n in nexts {
            if !visited.contains(n) && dfs_cycle(n, adj, visited, stack) {
                return true;
            }
            if stack.contains(n) {
                return true;
            }
        }
    }

    stack.remove(node);
    false
}
