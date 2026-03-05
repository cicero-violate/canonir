//! Deterministic emit pipeline integration
//! This module introduces validation, dependency ordering, and canonical
//! module layout normalization before Rust sources are written to disk.

use std::collections::{HashMap, HashSet};

/// Represents a dependency graph between IR items
pub type DependencyGraph = HashMap<String, HashSet<String>>;

/// Compute deterministic topological ordering
pub fn compute_emit_order(graph: &DependencyGraph) -> Vec<String> {
    let mut incoming: HashMap<String, usize> = HashMap::new();
    let mut ordered = Vec::new();

    for (node, deps) in graph {
        incoming.entry(node.clone()).or_insert(0);
        for dep in deps {
            *incoming.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<String> = incoming
        .iter()
        .filter(|(_, &v)| v == 0)
        .map(|(k, _)| k.clone())
        .collect();

    queue.sort();

    while let Some(node) = queue.pop() {
        ordered.push(node.clone());

        if let Some(deps) = graph.get(&node) {
            for dep in deps {
                if let Some(v) = incoming.get_mut(dep) {
                    *v -= 1;
                    if *v == 0 {
                        queue.push(dep.clone());
                        queue.sort();
                    }
                }
            }
        }
    }

    ordered
}

/// Normalize module paths to canonical Rust layout
pub fn normalize_module_path(module: &str) -> String {
    module.replace("::", "/") + ".rs"
}

/// Validate emitted structure
pub fn validate_structure(items: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();

    for item in items {
        if seen.contains(item) {
            return Err(format!("duplicate definition detected: {}", item));
        }
        seen.insert(item.clone());
    }

    Ok(())
}

/// Emit pipeline entry point
pub fn deterministic_emit(graph: DependencyGraph, items: Vec<String>) -> Result<Vec<String>, String> {
    validate_structure(&items)?;

    let order = compute_emit_order(&graph);

    let mut result = Vec::new();

    for item in order {
        let path = normalize_module_path(&item);
        result.push(path);
    }

    Ok(result)
}
