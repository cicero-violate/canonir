//! Deterministic emission helpers for canon-projection
//!
//! These helpers normalize module trees and symbol exports before
//! emitted Rust files are written to disk. The logic is intentionally
//! pure and deterministic so that orchestration DAG nodes can invoke
//! them safely.

use std::collections::{BTreeMap, BTreeSet};

/// Normalize a module tree represented as path -> children.
/// Ensures deterministic ordering and removes duplicates.
pub fn normalize_module_tree(
    tree: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut normalized = BTreeMap::new();

    for (module, children) in tree {
        let mut set = BTreeSet::new();
        for c in children {
            set.insert(c.clone());
        }
        normalized.insert(module.clone(), set.into_iter().collect());
    }

    normalized
}

/// Generate deterministic crate-root exports for a module set.
pub fn generate_crate_exports(modules: &[String]) -> Vec<String> {
    let mut ordered: BTreeSet<String> = BTreeSet::new();
    for m in modules {
        ordered.insert(m.clone());
    }

    ordered
        .into_iter()
        .map(|m| format!("pub mod {};", m))
        .collect()
}

/// Normalize symbol export visibility.
pub fn normalize_symbol_exports(symbols: &[String]) -> Vec<String> {
    let mut ordered: BTreeSet<String> = BTreeSet::new();
    for s in symbols {
        ordered.insert(s.clone());
    }

    ordered.into_iter().collect()
}
