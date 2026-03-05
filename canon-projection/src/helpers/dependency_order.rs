//! Dependency Ordering Utilities
//!
//! Provides deterministic ordering for emitted Rust items based on
//! dependency relationships discovered during projection planning.
//! This prevents forward‑reference build failures and non‑deterministic
//! emission order.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Directed dependency graph used for ordering emitted items.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    edges: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            edges: BTreeMap::new(),
        }
    }

    /// Register a node.
    pub fn add_node(&mut self, node: impl Into<String>) {
        self.edges.entry(node.into()).or_default();
    }

    /// Add dependency edge: `a -> b` meaning `a` depends on `b`.
    pub fn add_dependency(&mut self, node: impl Into<String>, depends_on: impl Into<String>) {
        let node = node.into();
        let dep = depends_on.into();

        self.edges.entry(node.clone()).or_default().insert(dep.clone());
        self.edges.entry(dep).or_default();
    }

    /// Perform stable topological sort.
    pub fn topo_sort(&self) -> Vec<String> {
        let mut indeg: BTreeMap<String, usize> = BTreeMap::new();

        for (n, deps) in &self.edges {
            indeg.entry(n.clone()).or_insert(0);
            for d in deps {
                *indeg.entry(d.clone()).or_insert(0) += 1;
            }
        }

        let mut q: VecDeque<String> = indeg
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(n, _)| n.clone())
            .collect();

        let mut result = Vec::new();
        let mut indeg = indeg;

        while let Some(n) = q.pop_front() {
            result.push(n.clone());

            if let Some(deps) = self.edges.get(&n) {
                for d in deps {
                    if let Some(v) = indeg.get_mut(d) {
                        *v -= 1;
                        if *v == 0 {
                            q.push_back(d.clone());
                        }
                    }
                }
            }
        }

        result
    }
}

/// Order items deterministically according to dependency graph.
pub fn order_items(nodes: Vec<String>, deps: Vec<(String, String)>) -> Vec<String> {
    let mut g = DependencyGraph::new();

    for n in nodes {
        g.add_node(n);
    }

    for (a, b) in deps {
        g.add_dependency(a, b);
    }

    g.topo_sort()
}
