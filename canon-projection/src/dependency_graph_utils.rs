use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Debug)]
pub struct DependencyGraph {
    pub edges: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.edges.entry(from.into()).or_default().insert(to.into());
    }

    pub fn dependencies_of(&self, module: &str) -> Option<&BTreeSet<String>> {
        self.edges.get(module)
    }
}
