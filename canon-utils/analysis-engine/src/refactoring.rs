use crate::duplicates::DuplicateReport;
use crate::loader::{AnalysisGraph, EdgeKind};
use algorithms::graph::csr::Csr;
use algorithms::graph::reachability::reachability_gpu;
use algorithms::graph::scc_gpu::scc_gpu;
use algorithms::graph::scheduler_gpu::{deadlock_gpu, pack_ready_priority, ready_mask_gpu};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RefactoringReport {
    pub scc_count: usize,
    pub recursive_clusters: usize,
    pub ready_count: i32,
    pub deadlock: bool,
    pub duplicates: usize,
    pub type_reachable: usize,
}

pub fn analyze_refactoring(graph: &AnalysisGraph, duplicates: &DuplicateReport) -> RefactoringReport {
    let call = build_kind_csr(graph, EdgeKind::Call);
    let sccs = scc_gpu(&call);
    let recursive_clusters = sccs.iter().filter(|c| c.len() > 1).count();

    let (deps_offset, deps_flat) = build_deps(&call);
    let status = vec![0u8; call.vertex_count()];
    let (ready_mask, ready_count, _completed) = ready_mask_gpu(&status, &deps_offset, &deps_flat);
    let priorities = vec![1u16; ready_mask.len()];
    let _keys = pack_ready_priority(&ready_mask, &priorities);
    let deadlock = deadlock_gpu(&status, &deps_offset, &deps_flat);

    let uses = build_kind_csr(graph, EdgeKind::UsesType);
    let mut type_reachable = 0usize;
    for pair in &duplicates.pairs {
        let reached = reachability_gpu(&uses, &[pair.left as usize]);
        if reached.get(pair.right as usize).copied().unwrap_or(false) {
            type_reachable += 1;
        }
    }

    RefactoringReport {
        scc_count: sccs.len(),
        recursive_clusters,
        ready_count,
        deadlock,
        duplicates: duplicates.pairs.len(),
        type_reachable,
    }
}

fn build_kind_csr(graph: &AnalysisGraph, kind: EdgeKind) -> Csr {
    let mut adj = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        if e.kind == kind {
            adj[e.src as usize].push(e.dst as usize);
        }
    }
    Csr::from_adj(&adj)
}

fn build_deps(csr: &Csr) -> (Vec<i32>, Vec<i32>) {
    let mut offset = Vec::with_capacity(csr.vertex_count() + 1);
    let mut flat = Vec::with_capacity(csr.edge_count());
    offset.push(0i32);
    for u in 0..csr.vertex_count() {
        for &v in csr.neighbours(u) {
            flat.push(v);
        }
        offset.push(flat.len() as i32);
    }
    (offset, flat)
}
