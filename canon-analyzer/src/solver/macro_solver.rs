use crate::solver::csr_to_adj;
use algorithms::graph::topological_sort::topological_sort;
use anyhow::{bail, Result};
use canon::node::CanonNodeKind;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let macro_nodes: Vec<usize> = ir.nodes.iter().filter(|n| matches!(&n.kind, CanonNodeKind::MacroCall { .. })).map(|n| n.id.0 as usize).collect();

    if macro_nodes.is_empty() {
        return Ok(());
    }

    let v = ir.macro_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.macro_graph);
    let order = topological_sort(&adj);
    if order.len() != v {
        bail!("macro_solver: recursive macro expansion cycle detected in G_macro");
    }

    log::info!("macro_solver: {} macro call(s), expansion order is acyclic", macro_nodes.len());
    Ok(())
}
