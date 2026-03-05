use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct IrItem {
    pub module: String,
    pub code: String,
}

pub fn normalize_module_tree(items: &[IrItem]) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();

    for item in items {
        let m = item.module.trim();
        if m.is_empty() {
            modules.insert("root".to_string());
        } else {
            modules.insert(m.to_string());
        }
    }

    if modules.is_empty() {
        modules.insert("root".to_string());
    }

    modules
}

pub fn compute_emit_plan(items: &[IrItem]) -> BTreeMap<String, Vec<String>> {
    let mut plan: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for item in items {
        let module = if item.module.trim().is_empty() {
            "root".to_string()
        } else {
            item.module.clone()
        };

        plan.entry(module).or_default().push(item.code.clone());
    }

    plan
}

pub fn validate_emitted_rust_structure(items: &[IrItem]) -> Vec<String> {
    let mut errors = Vec::new();

    for (i, item) in items.iter().enumerate() {
        if item.code.trim().is_empty() {
            errors.push(format!("IrItem {} has empty code block", i));
        }

        if item.module.contains(' ') {
            errors.push(format!("IrItem {} module contains spaces: {}", i, item.module));
        }
    }

    errors
}
