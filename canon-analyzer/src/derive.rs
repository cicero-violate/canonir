use crate::graph::{
    call_graph::CallGraphBuilder, cfg_graph::CfgGraphBuilder, macro_graph::MacroGraphBuilder, module_graph::ModuleGraphBuilder, name_graph::NameGraphBuilder, region_graph::RegionGraphBuilder,
    type_graph::TypeGraphBuilder, value_graph::ValueGraphBuilder,
};
use anyhow::Result;
use canon::node::CanonId;
use canon::CanonIR;
use model::ir::{csr_graph::CsrGraph, edge::EdgeKind, node::NodeId};

pub fn derive(ir: &mut CanonIR) -> Result<()> {
    let v = ir.nodes.len();

    let mut module_b = ModuleGraphBuilder::new(v);
    let mut call_b = CallGraphBuilder::new(v);
    let mut name_b = NameGraphBuilder::new(v);
    let mut type_b = TypeGraphBuilder::new(v);
    let mut cfg_b = CfgGraphBuilder::new(v);
    let mut region_b = RegionGraphBuilder::new(v);
    let mut value_b = ValueGraphBuilder::new(v);
    let mut macro_b = MacroGraphBuilder::new(v);

    module_b.derive_from_ir(ir);
    call_b.derive_from_ir(ir);
    name_b.derive_from_ir(ir);
    type_b.derive_from_ir(ir);
    cfg_b.derive_from_ir(ir);
    region_b.derive_from_ir(ir);
    value_b.derive_from_ir(ir);
    macro_b.derive_from_ir(ir);

    ir.module_graph = merge_graph_edges(v, &ir.module_graph, module_b.edges());
    ir.call_graph = merge_graph_edges(v, &ir.call_graph, call_b.edges());
    ir.name_graph = merge_graph_edges(v, &ir.name_graph, name_b.edges());
    ir.type_graph = merge_graph_edges(v, &ir.type_graph, type_b.edges());
    ir.cfg_graph = merge_graph_edges(v, &ir.cfg_graph, cfg_b.edges());
    ir.region_graph = merge_graph_edges(v, &ir.region_graph, region_b.edges());
    ir.value_graph = merge_graph_edges(v, &ir.value_graph, value_b.edges());
    ir.macro_graph = merge_graph_edges(v, &ir.macro_graph, macro_b.edges());

    Ok(())
}

fn merge_graph_edges(v: usize, existing: &CsrGraph<CanonId, EdgeKind>, derived: &[(u32, u32, EdgeKind)]) -> CsrGraph<CanonId, EdgeKind> {
    let mut edges: Vec<(u32, u32, EdgeKind)> = Vec::new();

    for src_idx in 0..existing.vertex_count() {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in existing.neighbours(src_id) {
            edges.push((src_id.0, dst_id.0, edge.clone()));
        }
    }

    edges.extend(derived.iter().cloned());

    let node_ids: Vec<CanonId> = (0..v as u32).map(CanonId).collect();
    CsrGraph::from_edges(node_ids, edges)
}
