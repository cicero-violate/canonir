//! Const Evaluation Solver (S11).
//!
//! Variables:
//!   C          = { v | NodeKind::Const | NodeKind::Static }
//!   G_value    = ir.value_graph  (ConstDep edges)
//!   topo(G)    = topological order of G
//!
//! Equations:
//!   eval_order = topo(G_value)          — evaluation order for const folding
//!   ∃ cycle in G_value  =>  Err         — circular const dependency
//!
//! Current implementation: cycle detection only (full folding deferred).

use anyhow::{bail, Result};
use model::ir::{model_ir::ModelIR, node::NodeKind};
use crate::solver::csr_to_adj;
use algorithms::graph::topological_sort::topological_sort;

pub fn solve(ir: &ModelIR) -> Result<()> {
    // Collect const/static node ids for diagnostics.
    let const_nodes: Vec<usize> = ir.nodes.iter()
        .filter(|n| matches!(&n.kind, NodeKind::Const { .. } | NodeKind::Static { .. }))
        .map(|n| n.id.index())
        .collect();

    if const_nodes.is_empty() {
        return Ok(());
    }

    // Build adjacency from G_value and check for cycles via topo-sort.
    // Equation: topo(G_value) succeeds <=> G_value is a DAG.
    let v = ir.value_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }
    let adj = csr_to_adj(&ir.value_graph);
    let order = topological_sort(&adj);
    if order.len() != v {
        bail!("const_solver: circular ConstDep dependency detected (cycle in G_value)");
    }

    log::info!("const_solver: {} const/static node(s) validated, eval order is acyclic", const_nodes.len());
    Ok(())
}
