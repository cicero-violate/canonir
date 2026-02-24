//! Module-graph solver — containment ordering via topological sort.
//!
//! Variables:
//!   G_module = ir.module_graph        (CSR, Contains edges)
//!   order    = topological_sort(adj)  — root modules before leaf items
//!
//! Equations:
//!   order = topo(G_module)
//!   // order[i] before order[j] => order[i] does not depend on order[j]
//!   // i.e. parent module always precedes its children in emit order
//!
//! Algorithm used: topological_sort (Kahn's, from algorithms crate)

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use algorithms::graph::topological_sort::topological_sort;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.module_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj   = csr_to_adj(&ir.module_graph);
    let order = topological_sort(&adj);

    // Store emit order on ModelIR so projection can consume it directly.
    ir.emit_order = order.into_iter().map(|i| model::ir::node::NodeId(i as u32)).collect();

    Ok(())
}
