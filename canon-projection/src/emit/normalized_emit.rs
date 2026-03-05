use std::collections::{HashMap, HashSet};

use crate::helpers::dependency_graph::compute_dependency_graph;
use crate::helpers::module_normalization::{normalize_module_tree, IrModule};
use crate::helpers::structural_validation::{validate_symbols, StructuralValidationReport};

#[derive(Clone, Debug)]
pub struct IrItem {
    pub id: String,
    pub deps: Vec<String>,
    pub symbol: String,
}

pub struct EmitInput {
    pub modules: Vec<IrModule>,
    pub items: Vec<IrItem>,
    pub defined_symbols: HashSet<String>,
    pub referenced_symbols: HashSet<String>,
}

pub struct EmitResult {
    pub ordered_items: Vec<String>,
    pub validation: StructuralValidationReport,
}

fn topo_sort(graph: &HashMap<String, HashSet<String>>) -> Vec<String> {
    let mut indegree: HashMap<String, usize> = HashMap::new();

    for (node, edges) in graph {
        indegree.entry(node.clone()).or_insert(0);
        for dep in edges {
            *indegree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| k.clone())
        .collect();

    queue.sort();

    let mut result = Vec::new();

    while let Some(node) = queue.pop() {
        result.push(node.clone());

        if let Some(edges) = graph.get(&node) {
            for dep in edges {
                if let Some(v) = indegree.get_mut(dep) {
                    *v -= 1;
                    if *v == 0 {
                        queue.push(dep.clone());
                    }
                }
            }
        }

        queue.sort();
    }

    result
}

pub fn emit_with_validation(input: EmitInput) -> EmitResult {
    // 1. Normalize module tree
    let _normalized_modules = normalize_module_tree(&input.modules);

    // 2. Build dependency graph
    let edges: Vec<(String, String)> = input
        .items
        .iter()
        .flat_map(|item| item.deps.iter().map(move |d| (item.id.clone(), d.clone())))
        .collect();

    let graph = compute_dependency_graph(&edges);

    // 3. Deterministic topological ordering
    let ordered = topo_sort(&graph);

    // 4. Structural validation
    let validation = validate_symbols(&input.defined_symbols, &input.referenced_symbols);

    EmitResult {
        ordered_items: ordered,
        validation,
    }
}
