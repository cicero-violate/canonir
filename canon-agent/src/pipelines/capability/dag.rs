use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::capability::{assert_class_disjoint, Capability, CapabilityClass};
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
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub budget: Option<u32>,
    #[serde(default)]
    pub reasoning_trace: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub readonly_fail_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
    #[serde(skip, default)]
    pub id_index: HashMap<String, usize>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self { nodes: Vec::new(), id_index: HashMap::new() }
    }

    pub fn add_node(&mut self, node: TaskNode) {
        let idx = self.nodes.len();
        self.id_index.insert(node.id.clone(), idx);
        self.nodes.push(node);
    }

    pub fn rebuild_index(&mut self) {
        self.id_index.clear();
        for (idx, node) in self.nodes.iter().enumerate() {
            self.id_index.insert(node.id.clone(), idx);
        }
    }

    fn ensure_index(&mut self) {
        if self.id_index.len() != self.nodes.len() {
            self.rebuild_index();
        }
    }

    pub fn get_node(&mut self, id: &str) -> Option<&TaskNode> {
        self.ensure_index();
        let idx = *self.id_index.get(id)?;
        self.nodes.get(idx)
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.ensure_index();
        let idx = *self.id_index.get(id)?;
        self.nodes.get_mut(idx)
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
            assert_class_disjoint(&caps).map_err(|e| format!("node {}: {}", n.id, e))?;
        }
        detect_cycle(self)?;
        Ok(())
    }

    pub fn reset_for_execution(&mut self) {
        for node in &mut self.nodes {
            node.status = Status::Pending;
            node.result = None;
            node.error = None;
            node.readonly_fail_count = 0;
        }
        self.rebuild_index();
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
        assert_class_disjoint(&caps)?;
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
        self.capabilities.iter().any(|c| c.class() == CapabilityClass::Verify)
    }

    pub fn is_mutation_context(&self) -> bool {
        self.capabilities.iter().any(|c| c.class() == CapabilityClass::Mutate)
    }
}

pub fn resolve_ready(graph: &mut TaskGraph) {
    let completed: std::collections::HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.status == Status::Completed)
        .map(|n| n.id.clone())
        .collect();
    for node in &mut graph.nodes {
        if node.status != Status::Pending {
            continue;
        }
        let deps_satisfied = node.deps.iter().all(|d| completed.contains(d));
        if deps_satisfied {
            node.status = PENDING_TO_READY[node.status as usize];
        }
    }
}

pub fn grant_authority(node: &TaskNode) -> Result<AuthorityContext, String> {
    let caps: std::collections::HashSet<Capability> = node.required_capabilities.iter().copied().collect();
    AuthorityContext::new(node.id.clone(), caps)
}
