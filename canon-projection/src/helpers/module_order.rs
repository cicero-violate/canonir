use std::collections::{BTreeMap, BTreeSet};

/// Maintains dependency ordering between emitted modules.
#[derive(Default, Debug)]
pub struct ModuleOrder {
    deps: BTreeMap<String, BTreeSet<String>>,
}

impl ModuleOrder {
    pub fn add_dependency(&mut self, module: impl Into<String>, depends_on: impl Into<String>) {
        let m = module.into();
        let d = depends_on.into();
        self.deps.entry(m).or_default().insert(d);
    }

    /// Produce a deterministic topological ordering.
    pub fn order(&self) -> Vec<String> {
        let mut visited = BTreeSet::new();
        let mut out = Vec::new();

        for m in self.deps.keys() {
            self.visit(m, &mut visited, &mut out);
        }

        out
    }

    fn visit(&self, module: &str, visited: &mut BTreeSet<String>, out: &mut Vec<String>) {
        if visited.contains(module) {
            return;
        }

        visited.insert(module.to_string());

        if let Some(children) = self.deps.get(module) {
            for dep in children {
                self.visit(dep, visited, out);
            }
        }

        out.push(module.to_string());
    }
}
