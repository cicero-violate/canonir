use crate::smt::loader::{AnalysisGraph, EdgeKind};
use algorithms::control_flow::gpu::{dominators_gpu, reaching_definitions_gpu};
use algorithms::graph::csr::Csr;
use algorithms::graph::model_checking::model_check_gpu;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct InvariantReport {
    pub dominator_words: usize,
    pub reaching_defs_words: usize,
    pub model_check_ok: bool,
}

pub fn analyze_invariants(graph: &AnalysisGraph) -> InvariantReport {
    let flow = build_kind_csr(graph, EdgeKind::Flow);
    let pred = reverse_csr(&flow);
    let node_count = flow.vertex_count();

    let dom = dominators_gpu(&pred, 0, node_count);
    let dominator_words = dom.len();

    let def_count = node_count.max(1);
    let words = (def_count + 63) / 64;
    let r#gen = vec![0u64; node_count * words];
    let kill = vec![0u64; node_count * words];
    let reaching = reaching_definitions_gpu(&pred, node_count, def_count, &r#gen, &kill);
    let reaching_defs_words = reaching.len();

    let invariant_mask = vec![1u8; node_count];
    let model_check_ok = model_check_gpu(&flow, &[0], &invariant_mask);

    InvariantReport {
        dominator_words,
        reaching_defs_words,
        model_check_ok,
    }
}

fn map_id(graph: &AnalysisGraph, id: u32) -> Option<usize> {
    graph.id_to_index.get(&id).copied()
}

fn build_kind_csr(graph: &AnalysisGraph, kind: EdgeKind) -> Csr {
    let mut adj = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        if e.kind == kind {
            if let (Some(src), Some(dst)) = (map_id(graph, e.src), map_id(graph, e.dst)) {
                adj[src].push(dst);
            }
        }
    }
    Csr::from_adj(&adj)
}

fn reverse_csr(csr: &Csr) -> Csr {
    let mut adj = vec![Vec::new(); csr.vertex_count()];
    for u in 0..csr.vertex_count() {
        for &v in csr.neighbours(u) {
            adj[v as usize].push(u);
        }
    }
    Csr::from_adj(&adj)
}
