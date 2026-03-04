use crate::dag::{NodeId, TaskGraph};
use std::collections::{HashSet, VecDeque};

/// Detect cycles using Kahn-style traversal.
pub fn check_no_cycles(graph: &TaskGraph) -> bool {
    let mut indegree = std::collections::HashMap::new();

    for node in &graph.nodes {
        indegree.entry(node.id.clone()).or_insert(0);
        for dep in &node.deps {
            *indegree.entry(node.id.clone()).or_insert(0) += 1;
            indegree.entry(dep.clone()).or_insert(0);
        }
    }

    let mut queue: VecDeque<NodeId> = indegree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(k, _)| k.clone())
        .collect();

    let mut visited = 0usize;

    while let Some(n) = queue.pop_front() {
        visited += 1;

        if let Some(node) = graph.get_node(&n) {
            for dep in &node.deps {
                if let Some(v) = indegree.get_mut(dep) {
                    *v -= 1;
                    if *v == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    visited == indegree.len()
}

/// Ensure all dependencies reference valid nodes.
pub fn check_dependency_consistency(graph: &TaskGraph) -> bool {
    let ids: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id.clone()).collect();

    for node in &graph.nodes {
        for dep in &node.deps {
            if !ids.contains(dep) {
                return false;
            }
        }
    }

    true
}
