use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::capability::{assert_mut_verify_disjoint, Capability};
use super::decompose::NodeType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Blocked,
}

const PENDING_TO_READY: [Status; 6] = [
    Status::Ready,     // Pending (0) -> Ready
    Status::Ready,     // Ready (1)
    Status::Running,   // Running (2)
    Status::Completed, // Completed (3)
    Status::Failed,    // Failed (4)
    Status::Blocked,   // Blocked (5)
];

const TRANSITION_TABLE: [[bool; 6]; 6] = {
    let mut t = [[false; 6]; 6];
    t[0][1] = true; // Pending -> Ready
    t[0][5] = true; // Pending -> Blocked
    t[1][2] = true; // Ready -> Running
    t[2][1] = true; // Running -> Ready
    t[2][3] = true; // Running -> Completed
    t[2][4] = true; // Running -> Failed
    t[5][1] = true; // Blocked -> Ready
    t
};

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
    TRANSITION_TABLE[from as usize][to as usize]
}

fn detect_cycle(graph: &TaskGraph) -> Result<(), String> {
    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let adj: Vec<Vec<usize>> = graph
        .nodes
        .iter()
        .map(|n| n.deps.iter().filter_map(|d| id_to_idx.get(d.as_str()).copied()).collect())
        .collect();
    let sccs = algorithms::graph::scc::kosaraju_scc(&adj);
    if sccs.iter().any(|c| c.len() > 1) {
        return Err("cycle detected in task graph".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct AuthorityContext {
    pub node_id: String,
    pub capabilities: HashSet<Capability>,
}

impl AuthorityContext {
    pub fn new(node_id: String, caps: HashSet<Capability>) -> Result<Self, String> {
        assert_mut_verify_disjoint(&caps)?;
        Ok(Self { node_id, capabilities: caps })
    }

    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn require(&self, cap: Capability) -> Result<(), String> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(format!("node {} missing capability {:?}", self.node_id, cap))
        }
    }

    pub fn is_verify_context(&self) -> bool {
        self.capabilities.contains(&Capability::StatusUpdateOnly)
    }

    pub fn is_mutation_context(&self) -> bool {
        self.capabilities.contains(&Capability::FileWrite) || self.capabilities.contains(&Capability::ApplyPatch)
    }
}

pub fn resolve_ready(graph: &mut TaskGraph) {
    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let adj: Vec<Vec<usize>> = graph
        .nodes
        .iter()
        .map(|n| {
            n.deps
                .iter()
                .filter_map(|d| id_to_idx.get(d.as_str()).copied())
                .collect()
        })
        .collect();
    let layers = algorithms::graph::scheduling::topological_layers(&adj);
    for &idx in layers.first().into_iter().flatten() {
        graph.nodes[idx].status = PENDING_TO_READY[graph.nodes[idx].status as usize];
    }
}

pub fn grant_authority(node: &TaskNode) -> Result<AuthorityContext, String> {
    let caps: std::collections::HashSet<Capability> = node.required_capabilities.iter().copied().collect();
    AuthorityContext::new(node.id.clone(), caps)
}
