//! Solver — Phase 2 of the analysis pipeline.
//!
//! Variables:
//!   ir : &mut ModelIR   — mutated in place by each sub-solver
//!
//! Pipeline:
//!   solve(ir)
//!     -> name_solver::solve(ir)    — topo order + rename propagation
//!     -> type_solver::solve(ir)    — SCC-based unification
//!     -> call_solver::solve(ir)    — DFS reachability on call graph
//!     -> module_solver::solve(ir)  — topo order containment validation
//!     -> cfg_solver::solve(ir)     — dominators / unreachable block removal

use anyhow::Result;
use model::ir::model_ir::ModelIR;

pub mod call_solver;
pub mod cfg_solver;
pub mod module_solver;
pub mod name_solver;
pub mod type_solver;
pub mod use_solver;
pub mod invariant_solver;
pub mod visibility_solver;
pub mod impl_solver;
pub mod trait_solver;
pub mod generic_solver;
pub mod provenance_solver;
pub mod cycle_diag_solver;
pub mod liveness_solver;
pub mod stability_solver;
pub mod borrow_solver;
pub mod const_solver;
pub mod macro_solver;
pub mod exhaustiveness_solver;
pub mod drop_solver;
pub mod unsafe_solver;

/// Run all solvers in dependency order.
pub fn solve(ir: &mut ModelIR) -> Result<()> {
    module_solver::solve(ir)?;   // containment must be settled first
    name_solver::solve(ir)?;     // rename constraints depend on module order
    type_solver::solve(ir)?;     // type unification depends on resolved names
    call_solver::solve(ir)?;     // call reachability depends on resolved types
    cfg_solver::solve(ir)?;      // CFG dominators depend on call resolution
    use_solver::solve(ir)?;      // inject Use nodes after all names are resolved
    // ── Phase 2: semantic correctness ───────────────────────────────────────
    invariant_solver::solve(ir)?; // structural safety (edges, impl targets, acyclicity)
    visibility_solver::solve(ir)?;// pub/private access rule enforcement
    impl_solver::solve(ir)?;      // impl target existence + duplicate detection
    trait_solver::solve(ir)?;     // trait method completeness
    generic_solver::solve(ir)?;   // TypeUnifies concrete conflict detection
    provenance_solver::solve(ir)?;// name shadowing + symbol origin chains
    cycle_diag_solver::solve(ir)?;// structured diagnostics for type SCC cycles
    liveness_solver::solve(ir)?;  // prune dead functions from emit_order
    stability_solver::solve(ir)?; // deterministic emit_order sort
    // ── Phase 3: advanced (stubs — active once IR gaps E5/E6/E12/E14 land) ─
    borrow_solver::solve(ir)?;
    const_solver::solve(ir)?;
    macro_solver::solve(ir)?;
    exhaustiveness_solver::solve(ir)?;
    drop_solver::solve(ir)?;
    unsafe_solver::solve(ir)?;
    Ok(())
}

// ── shared helper ────────────────────────────────────────────────────────────

/// Build a plain adjacency list from any CsrGraph for algorithm crate consumption.
///
/// Variables:
///   V = graph.vertex_count()
///   adj[v] = [dst.index() for (dst, _) in graph.neighbours(v)]
///
/// Equation:
///   adj[v] = col_idx[ row_ptr[v] .. row_ptr[v+1] ]  (cast to usize)
pub(crate) fn csr_to_adj<ND, ED>(graph: &model::ir::csr_graph::CsrGraph<ND, ED>) -> Vec<Vec<usize>> {
    let v = graph.vertex_count();
    (0..v)
        .map(|i| {
            graph
                .neighbours(model::ir::node::NodeId(i as u32))
                .map(|(dst, _)| dst.index())
                .collect()
        })
        .collect()
}
