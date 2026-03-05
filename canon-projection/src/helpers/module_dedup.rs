use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub module: String,
    pub file: String,
}

/// Remove duplicate module entries while preserving order.
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
