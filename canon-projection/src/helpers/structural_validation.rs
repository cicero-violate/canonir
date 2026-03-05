use std::collections::{HashMap, HashSet};

pub type ItemId = String;

#[derive(Debug)]
pub enum StructuralError {
    DuplicateDefinition(ItemId),
    UnresolvedDependency(ItemId),
}

/// Validate emitted structure
pub fn validate_emitted_rust_structure(items: &[ItemId], dependencies: &HashMap<ItemId, HashSet<ItemId>>) -> Vec<StructuralError> {
    let mut errors = Vec::new();

    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item.clone()) {
            errors.push(StructuralError::DuplicateDefinition(item.clone()));
        }
    }

    for (item, deps) in dependencies {
        for dep in deps {
            if !items.contains(dep) {
                errors.push(StructuralError::UnresolvedDependency(dep.clone()));
            }
        }
    }

    errors
}
