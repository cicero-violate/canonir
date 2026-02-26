use crate::solver::csr_to_adj;
use algorithms::graph::topological_sort::topological_sort;
use anyhow::{bail, Result};
use canon::node::CanonNodeKind;
use canon::CanonIR;

pub fn solve(ir: &CanonIR) -> Result<()> {
    let const_nodes: Vec<usize> = ir.nodes.iter().filter(|n| matches!(&n.kind, CanonNodeKind::Const { .. } | CanonNodeKind::Static { .. })).map(|n| n.id.0 as usize).collect();

    if const_nodes.is_empty() {
        return Ok(());
    }

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
