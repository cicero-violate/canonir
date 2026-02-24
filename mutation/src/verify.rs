//! verify — re-run analyze() + invariant_solver on a mutated IR.
//!
//! Variables:
//!   ir  : &ModelIR   — snapshot to verify (read-only view)
//!
//! Equation:
//!   verify(IR) = analyze(clone(IR)) ∧ invariant_solver(IR)
//!   // clone so verify is non-destructive

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use analyzer::solver::invariant_solver;

/// Verify structural and semantic invariants on `ir`.
/// Clones `ir` to run `analyze()` non-destructively, then runs
/// `invariant_solver` on the original for cheap structural checks.
///
/// Equation:
///   verify(IR) = analyze(IR.clone()) returns Ok
///              ∧ invariant_solver(IR) returns Ok
pub fn verify(ir: &ModelIR) -> Result<()> {
    // Full analysis on a clone — derives graphs + runs all solvers.
    // Equation: analyze(IR') where IR' = clone(IR)
    let mut scratch = ir.clone();
    analyzer::analyze(&mut scratch)?;

    // Cheap structural invariant on original (no clone needed — read-only).
    invariant_solver::solve(ir)?;

    Ok(())
}
