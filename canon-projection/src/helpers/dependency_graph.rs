use std::collections::{HashMap, HashSet};

pub type ItemId = String;

/// adjacency list representation
pub type DependencyGraph = HashMap<ItemId, HashSet<ItemId>>;

/// Pure function computing dependency graph from IR items
pub fn compute_dependency_graph(items: &[(ItemId, Vec<ItemId>)]) -> DependencyGraph {
    let mut graph: DependencyGraph = HashMap::new();

    for (item, deps) in items {
        let entry = graph.entry(item.clone()).or_insert_with(HashSet::new);
        for d in deps {
            entry.insert(d.clone());
        }
    }

    graph
}

/// Deterministic topological sort
pub fn compute_emit_order(graph: &DependencyGraph) -> Vec<ItemId> {
    let mut indegree: HashMap<ItemId, usize> = HashMap::new();

    for (node, deps) in graph {
        indegree.entry(node.clone()).or_insert(0);
        for dep in deps {
            *indegree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<ItemId> = indegree.iter().filter(|(_, deg)| **deg == 0).map(|(n, _)| n.clone()).collect();

    queue.sort();

    let mut order = Vec::new();

    while let Some(node) = queue.pop() {
        order.push(node.clone());

        if let Some(children) = graph.get(&node) {
            for c in children {
                if let Some(v) = indegree.get_mut(c) {
                    *v -= 1;
                    if *v == 0 {
                        queue.push(c.clone());
                    }
                }
            }
        }

        queue.sort();
    }

    order
}
