//! DAG Runtime Skeleton
//! Deterministic DAG scheduler and kernel interface.
//! All execution logic is expressed as pure kernel functions.

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
    pub status: NodeStatus,
}

#[derive(Debug, Clone)]
pub struct DagGraph {
    pub nodes: HashMap<NodeId, DagNode>,
}

impl DagGraph {
    pub fn new(nodes: Vec<DagNode>) -> Self {
        let map = nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
        Self { nodes: map }
    }
}

// ------------------------------------------------------------
// Kernel: compute_ready_nodes
// ------------------------------------------------------------

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

// ------------------------------------------------------------
// Kernel: compute_priority
// ------------------------------------------------------------

pub fn compute_priority(node_id: &NodeId) -> u64 {
    // deterministic placeholder priority
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let mut h = DefaultHasher::new();
    node_id.hash(&mut h);
    h.finish()
}

// ------------------------------------------------------------
// Kernel: schedule_next_node
// ------------------------------------------------------------

pub fn schedule_next_node(ready: &[NodeId]) -> Option<NodeId> {
    ready
        .iter()
        .min_by_key(|id| compute_priority(id))
        .cloned()
}

// ------------------------------------------------------------
// Scheduler
// ------------------------------------------------------------

pub fn schedule_batch(graph: &DagGraph) -> Vec<NodeId> {
    let ready = compute_ready_nodes(graph);

    let mut ready_sorted = ready.clone();
    ready_sorted.sort_by_key(|id| compute_priority(id));

    ready_sorted
}

// ------------------------------------------------------------
// Invariant Validation
// ------------------------------------------------------------

pub fn validate_acyclic(graph: &DagGraph) -> bool {
    fn visit(
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

        let ok = graph
            .nodes
            .get(node)
            .map(|n| {
                n.deps.iter().all(|d| visit(d, graph, visiting, visited))
            })
            .unwrap_or(false);

        visiting.remove(node);
        visited.insert(node.clone());

        ok
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    graph
        .nodes
        .keys()
        .all(|id| visit(id, graph, &mut visiting, &mut visited))
}

// ------------------------------------------------------------
// Runtime Execution Loop
// ------------------------------------------------------------

pub fn run_dag(graph: &mut DagGraph) {
    assert!(validate_acyclic(graph), "DAG invariant violated: graph contains cycle");

    loop {
        let ready = compute_ready_nodes(graph);

        if ready.is_empty() {
            break;
        }

        let batch = schedule_batch(graph);

        for id in batch {
            if let Some(node) = graph.nodes.get_mut(&id) {
                node.status = NodeStatus::Running;

                // placeholder execution
                node.status = NodeStatus::Completed;
            }
        }
    }
}
