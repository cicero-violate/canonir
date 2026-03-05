//! Emit guard layer
//! Integrates deterministic ordering and validation kernels
//! before Rust source files are written to disk.
//!
//! This module is intentionally side‑effect free except for
//! returning validated and ordered emit plans. File IO remains
//! in the caller.

use std::collections::{HashMap, HashSet, VecDeque};

pub type DependencyGraph = HashMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct EmitPlan {
    pub ordered_items: Vec<String>
}

#[derive(Debug)]
pub struct StructuralError {
    pub message: String
}

/// Deterministic topological ordering
pub fn compute_emit_order(graph: &DependencyGraph) -> Result<Vec<String>, StructuralError> {
    let mut indegree: HashMap<String, usize> = HashMap::new();

    for (node, deps) in graph {
        indegree.entry(node.clone()).or_insert(0);
        for dep in deps {
            *indegree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut ready: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| n.clone())
        .collect();

    ready.sort();

    let mut queue: VecDeque<String> = ready.into_iter().collect();
    let mut ordered = Vec::new();

    while let Some(node) = queue.pop_front() {
        ordered.push(node.clone());

        if let Some(edges) = graph.get(&node) {
            for dep in edges {
                if let Some(v) = indegree.get_mut(dep) {
                    *v -= 1;
                    if *v == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != indegree.len() {
        return Err(StructuralError {
            message: "cycle detected in emit dependency graph".into()
        });
    }

    Ok(ordered)
}

/// Detect duplicate items
pub fn validate_unique_items(items: &[String]) -> Result<(), StructuralError> {
    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item) {
            return Err(StructuralError {
                message: format!("duplicate emitted item: {}", item)
            });
        }
    }

    Ok(())
}

/// Entry point used by projection emit pipeline
pub fn validate_and_plan_emit(graph: &DependencyGraph) -> Result<EmitPlan, StructuralError> {
    let ordered = compute_emit_order(graph)?;

    validate_unique_items(&ordered)?;

    Ok(EmitPlan {
        ordered_items: ordered
    })
}
