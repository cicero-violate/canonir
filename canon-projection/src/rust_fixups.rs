use std::collections::{BTreeMap, BTreeSet};

/// Represents a discovered Rust module and the file that defines it.
#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub module: String,
    pub file: String,
}

/// Tracks symbol -> module ownership used during emission ordering.
#[derive(Default, Debug)]
pub struct SymbolOwnership {
    map: BTreeMap<String, String>
}

impl SymbolOwnership {
    pub fn register(&mut self, symbol: impl Into<String>, module: impl Into<String>) {
        self.map.insert(symbol.into(), module.into());
    }

    pub fn owner(&self, symbol: &str) -> Option<&str> {
        self.map.get(symbol).map(|s| s.as_str())
    }
}

/// Maintains dependency ordering between emitted modules.
#[derive(Default, Debug)]
pub struct ModuleOrder {
    deps: BTreeMap<String, BTreeSet<String>>
}

impl ModuleOrder {
    pub fn add_dependency(&mut self, module: impl Into<String>, depends_on: impl Into<String>) {
        let m = module.into();
        let d = depends_on.into();
        self.deps.entry(m).or_default().insert(d);
    }

    /// Produces a stable emission ordering using a simple topological walk.
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

/// Ensures emitted module names match filesystem-safe Rust identifiers.
pub fn normalize_module_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Removes duplicate module entries that can occur during projection.
pub fn dedup_modules(list: Vec<ModuleFile>) -> Vec<ModuleFile> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for m in list {
        if seen.insert(m.module.clone()) {
            out.push(m);
        }
    }

    out
}
