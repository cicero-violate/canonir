#![feature(rustc_private)]

//! Canon capture (rustc frontend) — map-reduce projection into CanonIR.

extern crate capture_rustc;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use anyhow::Result;
use canon::ir::CanonIR;
use rustc_middle::ty::TyCtxt;

pub mod canon_assemble;

/// Entry point: capture a crate directly into CanonIR using the scalable pipeline.
pub fn capture(tcx: TyCtxt<'_>) -> Result<CanonIR> {
    let index = capture_rustc::index::build_index(tcx);

    // Map: project each DefId sequentially (rayon disabled due to TyCtxt !Sync).
    let partials: Vec<capture_rustc::Partial> = index.def_ids.iter().map(|d| capture_rustc::project::project_def(tcx, *d, &index)).collect();

    // Reduce: deterministic Canon assembly.
    Ok(canon_assemble::canon_assemble(tcx, &index, partials))
}
