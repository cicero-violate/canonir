//! Deterministic emit validation and ordering kernels
//!
//! These functions enforce structural invariants before Rust source
//! emission occurs. They are pure kernels and introduce no side effects.

use std::collections::{HashMap, HashSet, VecDeque};

/// Dependency graph represented as adjacency list
pub type DependencyGraph = HashMap<String, Vec<String>>;

/// Computes deterministic topological ordering for emission.
///
/// Stable ordering rule:
/// - Nodes with no dependencies are emitted first
/// - Ties resolved lexicographically
pub fn compute_emit_order(graph: &DependencyGraph) -> Result<Vec<String>, String> {
    let mut indegree: HashMap<String, usize> = HashMap::new();

    for (node, deps) in graph {
        indegree.entry(node.clone()).or_insert(0);
        for dep in deps {
            *indegree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut ready: Vec<String> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(n, _)| n.clone())
        .collect();

    ready.sort();

    let mut queue: VecDeque<String> = ready.into_iter().collect();
    let mut ordered = Vec::new();

    while let Some(node) = queue.pop_front() {
        ordered.push(node.clone());

        if let Some(edges) = graph.get(&node) {
            for dep in edges {
                if let Some(entry) = indegree.get_mut(dep) {
                    *entry -= 1;
                    if *entry == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != indegree.len() {
        return Err("cycle detected in emit dependency graph".into());
    }

    Ok(ordered)
}

/// Validates structural correctness of emitted item set.
pub fn validate_items(items: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item) {
            return Err(format!("duplicate emitted item: {}", item));
        }
    }

    Ok(())
}
