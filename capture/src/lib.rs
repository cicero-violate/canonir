#![feature(rustc_private)]

//! Canon capture (rustc frontend) — map-reduce projection into ModelIR.
//!
//! Architecture (scalable / deterministic / parallel-ready):
//!   1) index  : stable DefId -> NodeId space (sorted)
//!   2) project: per-def projection (no shared mutation) producing Partial
//!   3) assemble: deterministic merge of Partials into ModelIR
//!
//! Future: swap rustc_private for public API when available; add incremental cache.

extern crate model;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

pub mod assemble;
pub mod index;
pub mod norm;
pub mod project;

/// Per-def capture output: nodes + edge hints (local to one DefId).
#[derive(Debug, Default)]
pub struct Partial {
    pub nodes: Vec<model::ir::node::Node>,
    pub edge_hints: Vec<model::ir::edge::EdgeHint>,
}

/// Entry point: capture a crate into ModelIR using the scalable pipeline.
pub fn capture(tcx: TyCtxt<'_>) -> Result<ModelIR> {
    let index = index::build_index(tcx);

    // Map: project each DefId sequentially (rayon disabled due to TyCtxt !Sync).
    let partials: Vec<Partial> = index.def_ids.iter().map(|d| project::project_def(tcx, *d, &index)).collect();

    // Reduce: deterministic assembly.
    let mut ir = assemble::assemble(tcx, index, partials);
    ir.cargo_dependencies = read_cargo_dependencies();
    Ok(ir)
}

/// Convenience for future incremental mode: project a single def.
pub fn capture_def(tcx: TyCtxt<'_>, def_id: DefId) -> Result<Partial> {
    Ok(project::project_def(tcx, def_id, &index::build_index(tcx)))
}

fn read_cargo_dependencies() -> Vec<String> {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let manifest = std::path::Path::new(&manifest_dir).join("Cargo.toml");
    let text = match std::fs::read_to_string(manifest) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut in_deps = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_deps = trimmed == "[dependencies]";
            continue;
        }
        if !in_deps || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}
