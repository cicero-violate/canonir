//! Filesystem layout normalization for projection emit
//! Ensures emitted crates follow canonical Rust layout:
//!
//! emit/<crate>/src/lib.rs
//! emit/<crate>/src/<module>.rs

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EmitPaths {
    pub crate_root: PathBuf,
    pub src_root: PathBuf
}

/// Compute canonical emit root for a crate
pub fn compute_emit_paths(base_emit_dir: &Path, crate_name: &str) -> EmitPaths {
    let crate_root = base_emit_dir.join(crate_name);
    let src_root = crate_root.join("src");

    EmitPaths {
        crate_root,
        src_root
    }
}

/// Normalize module file path inside emit/<crate>/src
pub fn module_file(src_root: &Path, module: &str) -> PathBuf {
    if module == "lib" {
        src_root.join("lib.rs")
    } else {
        src_root.join(format!("{}.rs", module))
    }
}

/// Convert IR module path to filesystem module path
pub fn normalize_module_path(src_root: &Path, module_segments: &[String]) -> PathBuf {
    if module_segments.is_empty() {
        return src_root.join("lib.rs");
    }

    if module_segments.len() == 1 {
        return module_file(src_root, &module_segments[0]);
    }

    let mut path = src_root.to_path_buf();

    for segment in &module_segments[..module_segments.len() - 1] {
        path = path.join(segment);
    }

    let leaf = &module_segments[module_segments.len() - 1];

    path.join(format!("{}.rs", leaf))
}
