use std::collections::{BTreeMap, BTreeSet};

/// Deterministically sort module names for stable emission order.
pub fn normalize_module_order<I: IntoIterator<Item = String>>(modules: I) -> Vec<String> {
    let mut set: BTreeSet<String> = modules.into_iter().collect();
    set.iter().cloned().collect()
}

/// Normalize symbol export map so exports are emitted deterministically.
pub fn normalize_symbol_exports<I: IntoIterator<Item = (String, String)>>(items: I) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (symbol, module) in items {
        map.insert(symbol, module);
    }
    map
}

/// Generate crate-root `pub use` lines deterministically from normalized exports.
pub fn generate_reexports(exports: &BTreeMap<String, String>) -> Vec<String> {
    exports
        .iter()
        .map(|(symbol, module)| format!("pub use crate::{}::{};", module, symbol))
        .collect()
}

/// Normalize module tree into deterministic representation.
pub fn normalize_module_tree<I: IntoIterator<Item = String>>(modules: I) -> Vec<String> {
    normalize_module_order(modules)
}
