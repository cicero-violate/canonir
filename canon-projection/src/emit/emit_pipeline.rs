//! Deterministic emit pipeline integrating validation, dependency ordering, and canonical module layout.

use std::collections::{HashMap, HashSet};

pub struct EmitItem {
    pub id: String,
    pub deps: Vec<String>,
    pub module_path: Vec<String>,
    pub code: String
}

pub struct EmitPlan {
    pub ordered_items: Vec<EmitItem>
}

/// Compute deterministic topological ordering
pub fn compute_emit_order(items: &[EmitItem]) -> Vec<EmitItem> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();

    fn visit(
        id: &str,
        items: &HashMap<String, EmitItem>,
        visited: &mut HashSet<String>,
        result: &mut Vec<EmitItem>
    ) {
        if visited.contains(id) {
            return;
        }

        if let Some(item) = items.get(id) {
            for dep in &item.deps {
                visit(dep, items, visited, result);
            }

            visited.insert(id.to_string());
            result.push(item.clone());
        }
    }

    let map: HashMap<String, EmitItem> = items.iter().map(|i| (i.id.clone(), i.clone())).collect();

    for item in items {
        visit(&item.id, &map, &mut visited, &mut result);
    }

    result
}

/// Validate structural constraints of emitted Rust items
pub fn validate_emitted_rust_structure(items: &[EmitItem]) -> Result<(), String> {
    let mut seen = HashSet::new();

    for item in items {
        if seen.contains(&item.id) {
            return Err(format!("duplicate definition detected: {}", item.id));
        }
        seen.insert(item.id.clone());
    }

    Ok(())
}

/// Build emit plan ensuring deterministic ordering and validated structure
pub fn compute_emit_plan(items: Vec<EmitItem>) -> Result<EmitPlan, String> {
    validate_emitted_rust_structure(&items)?;

    let ordered = compute_emit_order(&items);

    Ok(EmitPlan {
        ordered_items: ordered
    })
}
