//! Macro Expansion Solver (S12).
//!
//! Variables:
//!   M       = { v | NodeKind::MacroCall { path, tokens } }
//!   G_macro = ir.macro_graph  (Expands edges)
//!   topo(G) = topological order of G
//!
//! Equations:
//!   expand_order = topo(G_macro)     — safe expansion sequence
//!   ∃ cycle in G_macro  =>  Err     — recursive macro detected

use anyhow::{bail, Result};
use model::ir::{model_ir::ModelIR, node::NodeKind};
use crate::solver::csr_to_adj;
use algorithms::graph::topological_sort::topological_sort;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let macro_nodes: Vec<usize> = ir.nodes.iter()
        .filter(|n| matches!(&n.kind, NodeKind::MacroCall { .. }))
        .map(|n| n.id.index())
        .collect();

    if macro_nodes.is_empty() {
        return Ok(());
    }

    let v = ir.macro_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    // Equation: topo(G_macro) succeeds <=> no recursive macro expansion cycle.
    let adj = csr_to_adj(&ir.macro_graph);
    let order = topological_sort(&adj);
    if order.len() != v {
        bail!("macro_solver: recursive macro expansion cycle detected in G_macro");
    }

    log::info!("macro_solver: {} macro call(s), expansion order is acyclic", macro_nodes.len());
    Ok(())
}
