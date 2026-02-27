use crate::MutationOp;
use anyhow::{bail, Result};
use canon::csr_graph::CsrGraph;
use canon::{edge::EdgeKind, id::NodeId};
use canon::{
    node::{CanonId, CanonNodeKind},
    CanonIR, CanonNode,
};

fn graph_slot(kind: &EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Renames | EdgeKind::Resolves | EdgeKind::ImplRef | EdgeKind::Reexports => "name",
        EdgeKind::TypeOf | EdgeKind::TypeUnifies | EdgeKind::ImplTrait | EdgeKind::DynTrait | EdgeKind::Instantiates => "type",
        EdgeKind::Calls => "call",
        EdgeKind::Contains | EdgeKind::ImplFor | EdgeKind::AssocItem => "module",
        EdgeKind::CfgEdge | EdgeKind::CfgBranch { .. } => "cfg",
        EdgeKind::Outlives => "region",
        EdgeKind::ConstDep => "value",
        EdgeKind::Expands => "macro",
    }
}

fn extract_edges(graph: &CsrGraph<CanonId, EdgeKind>) -> Vec<(CanonId, CanonId, EdgeKind)> {
    let mut out = Vec::new();
    for src in 0..graph.vertex_count() {
        let src_id = NodeId(src as u32);
        for (dst, k) in graph.neighbours(src_id) {
            out.push((CanonId(src as u32), CanonId(dst.0), k.clone()));
        }
    }
    out
}

fn with_node_data_len(node_len: usize) -> Vec<CanonId> {
    (0..node_len as u32).map(CanonId).collect()
}

fn rebuild_graph(graph: &mut CsrGraph<CanonId, EdgeKind>, node_len: usize, edges: Vec<(CanonId, CanonId, EdgeKind)>) {
    let raw: Vec<(u32, u32, EdgeKind)> = edges.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect();
    *graph = CsrGraph::from_edges(with_node_data_len(node_len), raw);
}

fn strip_incident(edges: &mut Vec<(CanonId, CanonId, EdgeKind)>, id: CanonId) {
    edges.retain(|(s, d, _)| *s != id && *d != id);
}

fn shift_after_remove(edges: &mut Vec<(CanonId, CanonId, EdgeKind)>, removed: CanonId) {
    for (src, dst, _) in edges.iter_mut() {
        if src.0 > removed.0 {
            src.0 -= 1;
        }
        if dst.0 > removed.0 {
            dst.0 -= 1;
        }
    }
}

fn add_edge(ir: &mut CanonIR, src: CanonId, dst: CanonId, kind: EdgeKind) {
    let slot = graph_slot(&kind);
    let mut edges = match slot {
        "name" => extract_edges(&ir.name_graph),
        "type" => extract_edges(&ir.type_graph),
        "call" => extract_edges(&ir.call_graph),
        "module" => extract_edges(&ir.module_graph),
        "cfg" => extract_edges(&ir.cfg_graph),
        "region" => extract_edges(&ir.region_graph),
        "value" => extract_edges(&ir.value_graph),
        "macro" => extract_edges(&ir.macro_graph),
        _ => Vec::new(),
    };

    edges.push((src, dst, kind.clone()));

    let node_data = with_node_data_len(ir.nodes.len());
    let raw: Vec<(u32, u32, EdgeKind)> = edges.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect();
    match slot {
        "name" => ir.name_graph = CsrGraph::from_edges(node_data, raw),
        "type" => ir.type_graph = CsrGraph::from_edges(node_data, raw),
        "call" => ir.call_graph = CsrGraph::from_edges(node_data, raw),
        "module" => ir.module_graph = CsrGraph::from_edges(node_data, raw),
        "cfg" => ir.cfg_graph = CsrGraph::from_edges(node_data, raw),
        "region" => ir.region_graph = CsrGraph::from_edges(node_data, raw),
        "value" => ir.value_graph = CsrGraph::from_edges(node_data, raw),
        "macro" => ir.macro_graph = CsrGraph::from_edges(node_data, raw),
        _ => {}
    }
}

fn remove_edge(ir: &mut CanonIR, src: CanonId, dst: CanonId, kind: EdgeKind) {
    let slot = graph_slot(&kind);
    let mut edges = match slot {
        "name" => extract_edges(&ir.name_graph),
        "type" => extract_edges(&ir.type_graph),
        "call" => extract_edges(&ir.call_graph),
        "module" => extract_edges(&ir.module_graph),
        "cfg" => extract_edges(&ir.cfg_graph),
        "region" => extract_edges(&ir.region_graph),
        "value" => extract_edges(&ir.value_graph),
        "macro" => extract_edges(&ir.macro_graph),
        _ => Vec::new(),
    };

    edges.retain(|(s, d, k)| !(*s == src && *d == dst && *k == kind));

    let node_data = with_node_data_len(ir.nodes.len());
    let raw: Vec<(u32, u32, EdgeKind)> = edges.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect();
    match slot {
        "name" => ir.name_graph = CsrGraph::from_edges(node_data, raw),
        "type" => ir.type_graph = CsrGraph::from_edges(node_data, raw),
        "call" => ir.call_graph = CsrGraph::from_edges(node_data, raw),
        "module" => ir.module_graph = CsrGraph::from_edges(node_data, raw),
        "cfg" => ir.cfg_graph = CsrGraph::from_edges(node_data, raw),
        "region" => ir.region_graph = CsrGraph::from_edges(node_data, raw),
        "value" => ir.value_graph = CsrGraph::from_edges(node_data, raw),
        "macro" => ir.macro_graph = CsrGraph::from_edges(node_data, raw),
        _ => {}
    }
}

fn remove_node_reindex(ir: &mut CanonIR, id: CanonId) {
    ir.nodes.remove(id.0 as usize);
    for (idx, node) in ir.nodes.iter_mut().enumerate() {
        node.id = CanonId(idx as u32);
    }
    ir.emit_order.retain(|eid| *eid != id);
    for eid in &mut ir.emit_order {
        if eid.0 > id.0 {
            eid.0 -= 1;
        }
    }

    let mut graphs = vec![
        extract_edges(&ir.name_graph),
        extract_edges(&ir.type_graph),
        extract_edges(&ir.call_graph),
        extract_edges(&ir.module_graph),
        extract_edges(&ir.cfg_graph),
        extract_edges(&ir.region_graph),
        extract_edges(&ir.value_graph),
        extract_edges(&ir.macro_graph),
    ];

    for edges in &mut graphs {
        strip_incident(edges, id);
        shift_after_remove(edges, id);
    }

    let node_data = with_node_data_len(ir.nodes.len());
    let to_raw = |v: Vec<(CanonId, CanonId, EdgeKind)>| -> Vec<(u32, u32, EdgeKind)> { v.into_iter().map(|(s, d, k)| (s.0, d.0, k)).collect() };
    ir.name_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.type_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.call_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.module_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.cfg_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.region_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.value_graph = CsrGraph::from_edges(node_data.clone(), to_raw(graphs.remove(0)));
    ir.macro_graph = CsrGraph::from_edges(node_data, to_raw(graphs.remove(0)));

    ir.restore();
}

pub fn apply(ir: &mut CanonIR, op: MutationOp) -> Result<CanonId> {
    match op {
        MutationOp::AddNode { kind } => {
            let id = CanonId(ir.nodes.len() as u32);
            ir.nodes.push(CanonNode { id, kind });

            // Grow each graph's node_data while preserving edges.
            let node_len = ir.nodes.len();
            let edges = extract_edges(&ir.name_graph);
            rebuild_graph(&mut ir.name_graph, node_len, edges);
            let edges = extract_edges(&ir.type_graph);
            rebuild_graph(&mut ir.type_graph, node_len, edges);
            let edges = extract_edges(&ir.call_graph);
            rebuild_graph(&mut ir.call_graph, node_len, edges);
            let edges = extract_edges(&ir.module_graph);
            rebuild_graph(&mut ir.module_graph, node_len, edges);
            let edges = extract_edges(&ir.cfg_graph);
            rebuild_graph(&mut ir.cfg_graph, node_len, edges);
            let edges = extract_edges(&ir.region_graph);
            rebuild_graph(&mut ir.region_graph, node_len, edges);
            let edges = extract_edges(&ir.value_graph);
            rebuild_graph(&mut ir.value_graph, node_len, edges);
            let edges = extract_edges(&ir.macro_graph);
            rebuild_graph(&mut ir.macro_graph, node_len, edges);

            Ok(id)
        }
        MutationOp::RemoveNode { id } => {
            let idx = id.0 as usize;
            if idx >= ir.nodes.len() {
                bail!("apply::RemoveNode: CanonId {} out of range (|V|={})", idx, ir.nodes.len());
            }
            remove_node_reindex(ir, id);
            Ok(id)
        }
        MutationOp::UpdateNode { id, kind } => {
            let idx = id.0 as usize;
            if idx >= ir.nodes.len() {
                bail!("apply::UpdateNode: CanonId {} out of range (|V|={})", idx, ir.nodes.len());
            }
            ir.nodes[idx].kind = kind;
            ir.restore();
            Ok(id)
        }
        MutationOp::AddEdge { src, dst, kind } => {
            if src.0 as usize >= ir.nodes.len() {
                bail!("apply::AddEdge: src {} out of range", src.0);
            }
            if dst.0 as usize >= ir.nodes.len() {
                bail!("apply::AddEdge: dst {} out of range", dst.0);
            }
            add_edge(ir, src, dst, kind);
            Ok(src)
        }
        MutationOp::RemoveEdge { src, dst, kind } => {
            remove_edge(ir, src, dst, kind);
            Ok(src)
        }
    }
}
