use anyhow::Result;
use canon::ir::CanonIR;

pub mod derive;
pub mod graph;
pub mod solver;

pub fn canon_analyze(ir: &mut CanonIR) -> Result<()> {
    derive::derive(ir)?;
    solver::solve(ir)?;
    Ok(())
}
