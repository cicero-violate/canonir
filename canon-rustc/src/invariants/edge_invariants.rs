use anyhow::{bail, Result};
use canon_ir::ir::CanonIR;

pub fn validate_edge_endpoints(ir: &CanonIR) -> Result<()> {
    let node_count = ir.nodes.len();
    let graphs = [
        ("name_graph", &ir.name_graph),
        ("type_graph", &ir.type_graph),
        ("call_graph", &ir.call_graph),
        ("module_graph", &ir.module_graph),
        ("cfg_graph", &ir.cfg_graph),
        ("region_graph", &ir.region_graph),
        ("value_graph", &ir.value_graph),
        ("macro_graph", &ir.macro_graph),
    ];
    for (label, graph) in graphs {
        if graph.col_idx.iter().any(|dst| (*dst as usize) >= node_count) {
            bail!("Invariant violation: {label} edge endpoint out of bounds node_count={node_count}");
        }
    }
    if ir.graph_csr.col_idx.iter().any(|dst| (*dst as usize) >= node_count) {
        bail!("Invariant violation: graph_csr edge endpoint out of bounds node_count={node_count}");
    }
    if ir.graph_csr_rev.col_idx.iter().any(|dst| (*dst as usize) >= node_count) {
        bail!("Invariant violation: graph_csr_rev edge endpoint out of bounds node_count={node_count}");
    }
    Ok(())
}
