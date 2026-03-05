//! Module-tree normalization and emission planning helpers for Canon projection.
//!
//! These utilities support the Rust emitter by:
//! - Normalizing module paths
//! - Tracking symbol definitions
//! - Preventing duplicate definitions
//! - Ordering files for deterministic emission

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Represents a normalized module path like `a::b::c`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn from_path(path: &Path) -> Self {
        let parts = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .map(|s| s.replace(".rs", ""))
            .collect();
        ModulePath(parts)
    }

    pub fn to_string(&self) -> String {
        self.0.join("::")
    }
}

/// Tracks symbols to prevent duplicate definitions across emitted files.
#[derive(Default)]
pub struct SymbolTable {
    symbols: HashMap<String, PathBuf>,
}

impl SymbolTable {
    pub fn insert(&mut self, name: String, file: PathBuf) -> Result<(), String> {
        if let Some(existing) = self.symbols.get(&name) {
            return Err(format!(
                "duplicate symbol '{}' defined in {:?} and {:?}",
                name, existing, file
            ));
        }
        self.symbols.insert(name, file);
        Ok(())
    }
}

/// Represents an emission plan entry.
#[derive(Debug, Clone)]
pub struct EmitUnit {
    pub module: ModulePath,
    pub file_path: PathBuf,
}

/// Produces a deterministic ordering for emission.
pub fn order_emit_units(units: &mut Vec<EmitUnit>) {
    units.sort_by(|a, b| a.module.to_string().cmp(&b.module.to_string()));
}

/// Collect module declarations for a directory tree.
pub fn collect_modules(src_root: &Path) -> Vec<ModulePath> {
    let mut modules = Vec::new();

    if let Ok(entries) = std::fs::read_dir(src_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                modules.push(ModulePath::from_path(&path));
            }
        }
    }

    modules
}

/// Prevent duplicate file emission.
pub fn ensure_unique_files(paths: &[PathBuf]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for p in paths {
        if !seen.insert(p) {
            return Err(format!("duplicate emission target detected: {:?}", p));
        }
    }
    Ok(())
}
