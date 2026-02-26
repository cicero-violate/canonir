use anyhow::Result;
use model::ir::model_ir::ModelIR;

pub mod derive;
pub mod graph;
pub mod solver;

/// Unified analysis entry point.
///
/// Deterministic pipeline:
///   1. derive(&ModelIR) — build constraint graphs from node arena
///   2. solve(&mut ModelIR) — propagate constraints, annotate nodes
#[deprecated(note = "Model analyzer is legacy-only. Prefer canon-analyzer on CanonIR.")]
pub fn analyze(ir: &mut ModelIR) -> Result<()> {
    derive::derive(ir)?;
    solver::solve(ir)?;
    Ok(())
}
