//! Canon capture (rustc frontend) — map-reduce projection into CanonIR.

use anyhow::Result;
use canon_ir::ir::CanonIR;
use rustc_middle::ty::TyCtxt;

pub mod assembler;
pub mod pipeline;
pub mod index;
pub mod normalization;
pub mod spans;
pub mod types;

/// Per-def capture output: nodes + edge hints (local to one DefId).
#[derive(Debug, Default)]
pub struct Partial {
    pub nodes: Vec<types::Node>,
    pub edge_hints: Vec<types::EdgeHint>,
    pub panic_def_id: Option<String>,
}

/// Entry point: capture a crate directly into CanonIR using the scalable pipeline.
pub fn capture(tcx: TyCtxt<'_>) -> Result<CanonIR> {
    pipeline::pipeline::capture(tcx)
}

pub use spans::collect_spans_and_symbols;
pub use spans::{collect_symbol_spans, SpanInfo, SymbolSpanBundle};
