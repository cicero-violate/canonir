//! Emission normalization utilities for Canon projection.
//!
//! This module ensures emitted Rust crates obey Rust module-system
//! invariants before writing files to disk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Representation of a discovered module file.
#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub module_name: String,
    pub file_path: PathBuf,
}

/// Scan a directory for Rust module files and normalize them.
pub fn discover_modules(src_dir: &Path) -> Vec<ModuleFile> {
    let mut modules = Vec::new();

    if let Ok(entries) = std::fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem != "lib" && stem != "main" {
                        modules.push(ModuleFile {
                            module_name: stem.to_string(),
                            file_path: path,
                        });
                    }
                }
            }
        }
    }

    modules.sort_by(|a, b| a.module_name.cmp(&b.module_name));
    modules
}

/// Generate canonical `mod` declarations for a crate root.
pub fn generate_mod_block(modules: &[ModuleFile]) -> String {
    let mut out = String::new();

    for m in modules {
        out.push_str("mod ");
        out.push_str(&m.module_name);
        out.push_str(";\n");
    }

    out
}

/// Ensure no duplicate symbol definitions are emitted.
pub fn check_duplicate_symbols(symbols: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();

    for sym in symbols {
        if !seen.insert(sym) {
            return Err(format!("duplicate symbol emitted: {}", sym));
        }
    }

    Ok(())
}

/// Deterministic ordering of emitted files.
pub fn order_files(files: &mut Vec<PathBuf>) {
    files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
}

/// Build a module map keyed by module name.
pub fn build_module_map(modules: Vec<ModuleFile>) -> BTreeMap<String, PathBuf> {
    let mut map = BTreeMap::new();

    for m in modules {
        map.insert(m.module_name.clone(), m.file_path.clone());
    }

    map
}
