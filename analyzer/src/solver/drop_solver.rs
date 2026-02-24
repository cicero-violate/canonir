//! Drop Order Solver (S14) — stub pending ownership IR.
//!
//! Variables (future):
//!   scope_tree = CFG-based nesting of let bindings
//!   drop_order(scope) = reverse(declaration_order(scope))
//!
//! Equation (future):
//!   correct_drop <=> ∀ scope: drop_order consistent with CFG post-dominators
//!
//! Status: no-op until IR gains ownership/scope annotations.

use anyhow::Result;
use model::ir::model_ir::ModelIR;

pub fn solve(_ir: &ModelIR) -> Result<()> {
    // TODO(S14): implement when IR gains scope/ownership nodes.
    Ok(())
}
