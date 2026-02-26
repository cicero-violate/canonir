use anyhow::Result;
use canon::CanonIR;

pub fn verify(ir: &CanonIR) -> Result<()> {
    let mut scratch = ir.clone();
    canon_analyzer::canon_analyze(&mut scratch)?;
    canon_analyzer::solver::invariant_solver::solve(ir)?;
    Ok(())
}
