#![feature(rustc_private)]

//! Canon capture (rustc frontend) — map-reduce projection into CanonIR.

extern crate rustc_abi;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use anyhow::Result;
use canon::ir::CanonIR;
use rustc_middle::ty::TyCtxt;

pub mod canon_assemble;
pub mod capture;
pub mod index;
pub mod norm;
pub mod types;

/// Per-def capture output: nodes + edge hints (local to one DefId).
#[derive(Debug, Default)]
pub struct Partial {
    pub nodes: Vec<types::Node>,
    pub edge_hints: Vec<types::EdgeHint>,
}

/// Entry point: capture a crate directly into CanonIR using the scalable pipeline.
pub fn capture(tcx: TyCtxt<'_>) -> Result<CanonIR> {
    capture::pipeline::capture(tcx)
}
