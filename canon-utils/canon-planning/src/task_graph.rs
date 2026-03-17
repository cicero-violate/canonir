use super::capability_types::{capability_model_assert_class_disjoint, CapabilityMode, PipelineCapability};
use super::decompose::DecomposeNodeType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Blocked,
}

pub type Status = NodeStatus;
const PENDING_TO_READY: [NodeStatus; 6] = [NodeStatus::Ready, NodeStatus::Ready, NodeStatus::Running, NodeStatus::Completed, NodeStatus::Failed, NodeStatus::Blocked];
const TRANSITION_TABLE: [[bool; 6]; 6] = {
    let mut t = [[false; 6]; 6];
    t[0][1] = true;
    t[0][5] = true;
    t[1][2] = true;
    t[1][4] = true;
    t[2][1] = true;
    t[2][3] = true;
    t[2][4] = true;
    t[5][1] = true;
    t
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub description: String,
    pub status: NodeStatus,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<PipelineCapability>,
    #[serde(default)]
    pub node_type: DecomposeNodeType,
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
    #[serde(default)]
    pub repair_attempts: u32,
    #[serde(default)]
    pub completed_iter: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshotNode {
    pub id: String,
    pub description: String,
    pub node_type: DecomposeNodeType,
    pub deps: Vec<String>,
    pub required_capabilities: Vec<PipelineCapability>,
    pub status: NodeStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub causal_summary: Option<String>,
    #[serde(default)]
    pub failure_summary: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
    #[serde(skip, default)]
    pub id_index: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityNode {
    pub id: String,
    pub capability: PipelineCapability,
    #[serde(default)]
    pub deps: Vec<String>,
}

fn select_primary_capability(caps: &[PipelineCapability]) -> PipelineCapability {
    if caps.is_empty() {
        return PipelineCapability::Unknown;
    }
    let mut observe = None;
    let mut verify = None;
    let mut mutate = None;
    for &cap in caps {
        match cap.class() {
            CapabilityMode::Mutate => {
                mutate = mutate.or(Some(cap));
            }
            CapabilityMode::Verify => {
                verify = verify.or(Some(cap));
            }
            CapabilityMode::Observe => {
                observe = observe.or(Some(cap));
            }
        }
    }
    mutate.or(verify).or(observe).unwrap_or(caps[0])
}

impl From<&TaskNode> for CapabilityNode {
    fn from(node: &TaskNode) -> Self {
        let capability = select_primary_capability(&node.required_capabilities);
        Self {
            id: node.id.clone(),
            capability,
            deps: node.deps.clone(),
        }
    }
}

impl TaskNode {
    pub fn to_capability_node(&self) -> CapabilityNode {
        CapabilityNode::from(self)
    }
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
        self.nodes.iter().filter(|n| n.status == NodeStatus::Ready).collect()
    }
    pub fn all_completed(&self) -> bool {
        !self.nodes.is_empty() && self.nodes.iter().all(|n| n.status == NodeStatus::Completed)
    }
    pub fn has_failed(&self) -> bool {
        self.nodes.iter().any(|n| n.status == NodeStatus::Failed)
    }
    pub fn update_status(&mut self, id: &str, status: NodeStatus) -> Result<(), String> {
        let node = self.get_node_mut(id).ok_or_else(|| format!("unknown node id: {id}"))?;
        if !task_graph_transition_allowed(node.status, status) {
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
            if n.required_capabilities.is_empty() {
                return Err(format!("node {} missing required_capabilities", n.id));
            }
            let caps: HashSet<PipelineCapability> = n.required_capabilities.iter().copied().collect();
            capability_model_assert_class_disjoint(&caps).map_err(|e| format!("node {}: {}", n.id, e))?;
        }
        task_graph_detect_cycle(self)?;
        Ok(())
    }
    pub fn reset_for_execution(&mut self) {
        for node in &mut self.nodes {
            node.status = NodeStatus::Pending;
            node.result = None;
            node.error = None;
            node.readonly_fail_count = 0;
            node.completed_iter = None;
        }
        self.rebuild_index();
    }
}
fn task_graph_transition_allowed(from: NodeStatus, to: NodeStatus) -> bool {
    TRANSITION_TABLE[from as usize][to as usize]
}
fn task_graph_detect_cycle(graph: &TaskGraph) -> Result<(), String> {
    let id_to_idx: HashMap<&str, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let adj: Vec<Vec<usize>> = graph.nodes.iter().map(|n| n.deps.iter().filter_map(|d| id_to_idx.get(d.as_str()).copied()).collect()).collect();
    let sccs = algorithms::graph::scc::kosaraju_scc(&adj);
    if sccs.iter().any(|c| c.len() > 1) {
        return Err("cycle detected in task graph".into());
    }
    Ok(())
}
#[derive(Debug, Clone)]
pub struct NodeAuthority {
    pub node_id: String,
    pub capabilities: HashSet<PipelineCapability>,
}
impl NodeAuthority {
    pub fn new(node_id: String, caps: HashSet<PipelineCapability>) -> Result<Self, String> {
        capability_model_assert_class_disjoint(&caps)?;
        Ok(Self { node_id, capabilities: caps })
    }
    pub fn has(&self, cap: PipelineCapability) -> bool {
        self.capabilities.contains(&cap)
    }
    pub fn require(&self, cap: PipelineCapability) -> Result<(), String> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(format!("node {} missing capability {:?}", self.node_id, cap))
        }
    }
    pub fn is_verify_context(&self) -> bool {
        self.capabilities.iter().any(|c| c.class() == CapabilityMode::Verify)
    }
    pub fn is_mutation_context(&self) -> bool {
        self.capabilities.iter().any(|c| c.class() == CapabilityMode::Mutate)
    }
}
pub fn task_graph_resolve_ready(graph: &mut TaskGraph) {
    let completed: std::collections::HashSet<String> = graph.nodes.iter().filter(|n| n.status == NodeStatus::Completed).map(|n| n.id.clone()).collect();
    for node in &mut graph.nodes {
        if node.status != NodeStatus::Pending {
            continue;
        }
        let deps_satisfied = node.deps.iter().all(|d| completed.contains(d));
        if deps_satisfied {
            node.status = PENDING_TO_READY[node.status as usize];
        }
    }
}
pub fn task_graph_grant_authority(node: &TaskNode) -> Result<NodeAuthority, String> {
    let caps: std::collections::HashSet<PipelineCapability> = node.required_capabilities.iter().copied().collect();
    NodeAuthority::new(node.id.clone(), caps)
}
