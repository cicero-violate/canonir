//! Type-graph solver — unification via SCC detection.
//!
//! Variables:
//!   G_type  = ir.type_graph          (CSR)
//!   sccs    = kosaraju_scc(adj)       — groups of mutually-recursive types
//!
//! Equations:
//!   sccs = kosaraju(G_type)
//!   for scc in sccs where |scc| > 1:
//!     // all nodes in scc participate in a mutual type constraint cycle
//!     // mark for unification (future: emit error if conflicting concrete types)
//!
//! Algorithm used: Kosaraju SCC (from algorithms crate)

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use algorithms::graph::scc::kosaraju_scc;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj  = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    // Flag cyclic type groups (|scc| > 1 means mutual recursion).
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        // Future: emit diagnostics or unification constraints per cycle.
        let _ = scc;
    }

    Ok(())
}
