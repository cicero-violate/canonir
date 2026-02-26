use crate::solver::csr_to_adj;
use algorithms::graph::topological_sort::topological_sort;
use anyhow::Result;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.module_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.module_graph);
    let order = topological_sort(&adj);
    ir.emit_order = order.into_iter().map(|i| canon::node::CanonId(i as u32)).collect();
    Ok(())
}
