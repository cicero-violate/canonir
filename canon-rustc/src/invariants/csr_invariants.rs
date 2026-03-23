use anyhow::{bail, Result};
use canon_ir::ir::CanonIR;

pub fn validate_csr(ir: &CanonIR) -> Result<()> {
    if let Err(msg) = ir.validate_global_csr() {
        bail!("Invariant violation: {msg}");
    }
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
        if graph.row_ptr.len() != graph.node_data.len() + 1 {
            bail!(
                "Invariant violation: {label} row_ptr length mismatch row_ptr={} node_data={}",
                graph.row_ptr.len(),
                graph.node_data.len()
            );
        }
        if graph.node_data.len() != node_count {
            bail!(
                "Invariant violation: {label} node_data length mismatch node_data={} node_count={node_count}",
                graph.node_data.len()
            );
        }
        if graph.col_idx.len() != graph.edge_data.len() {
            bail!(
                "Invariant violation: {label} edge payload length mismatch col_idx={} edge_data={}",
                graph.col_idx.len(),
                graph.edge_data.len()
            );
        }
    }
    Ok(())
}
