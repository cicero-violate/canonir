use crate::smt::duplicates::DuplicateReport;
use crate::smt::loader::{AnalysisGraph, EdgeKind};
use algorithms::graph::csr::Csr;
use algorithms::graph::reachability::reachability_gpu;
use algorithms::graph::scc_gpu::scc_gpu;
use algorithms::graph::scheduler_gpu::{deadlock_gpu, pack_ready_priority, ready_mask_gpu};
use serde::Serialize;
use std::time::Instant;

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
    let t0 = Instant::now();
    let call = build_kind_csr(graph, EdgeKind::Call);
    let t_call = t0.elapsed();

    let t1 = Instant::now();
    let sccs = scc_gpu(&call);
    let t_scc = t1.elapsed();
    let recursive_clusters = sccs.iter().filter(|c| c.len() > 1).count();

    let t2 = Instant::now();
    let (deps_offset, deps_flat) = build_deps(&call);
    let status = vec![0u8; call.vertex_count()];
    let (ready_mask, ready_count, _completed) = ready_mask_gpu(&status, &deps_offset, &deps_flat);
    let priorities = vec![1u16; ready_mask.len()];
    let _keys = pack_ready_priority(&ready_mask, &priorities);
    let deadlock = deadlock_gpu(&status, &deps_offset, &deps_flat);
    let t_sched = t2.elapsed();

    let t3 = Instant::now();
    let uses = build_kind_csr(graph, EdgeKind::UsesType);
    let mut type_reachable = 0usize;
    for pair in &duplicates.pairs {
        if let (Some(src), Some(dst)) = (map_id(graph, pair.left), map_id(graph, pair.right)) {
            let reached = reachability_gpu(&uses, &[src]);
            if reached.get(dst).copied().unwrap_or(false) {
                type_reachable += 1;
            }
        }
    }
    let t_types = t3.elapsed();

    eprintln!(
        "refactoring timings: call_csr={:?} scc={:?} schedule={:?} uses+reachability={:?}",
        t_call, t_scc, t_sched, t_types
    );

    RefactoringReport {
        scc_count: sccs.len(),
        recursive_clusters,
        ready_count,
        deadlock,
        duplicates: duplicates.pairs.len(),
        type_reachable,
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
