use crate::smt::loader::{AnalysisGraph, EdgeKind, NodeKind};
use algorithms::graph::csr::Csr;
use algorithms::graph::feature_gpu::edge_kind_histogram_gpu;
use algorithms::graph::reachability::reachability_batched_gpu;
#[cfg(feature = "cuda")]
use algorithms::numerical::gpu::cosine_distance_gpu;
use anyhow::{anyhow, Result};
use serde::Serialize;

#[cfg(not(feature = "cuda"))]
compile_error!("analysis-engine duplicates requires feature \"cuda\"");

#[derive(Debug, Serialize)]
pub struct DuplicatePair {
    pub left: u32,
    pub right: u32,
    pub distance: f32,
    pub reachable: bool,
}

#[derive(Debug, Serialize)]
pub struct DuplicateReport {
    pub epsilon: f32,
    pub pairs: Vec<DuplicatePair>,
}

pub fn find_duplicates(graph: &AnalysisGraph, epsilon: f32) -> Result<DuplicateReport> {
    let kind_count = graph.edge_kinds.len().max(1);
    if kind_count > u8::MAX as usize {
        return Err(anyhow!("edge kind count exceeds u8 limit"));
    }
    let (csr, edge_kinds) = build_csr_with_kinds(graph)?;
    let counts = edge_kind_histogram_gpu(&csr, &edge_kinds, kind_count);

    let call_csr = build_kind_csr(graph, EdgeKind::Call);
    let rev_csr = reverse_csr(&call_csr);
    let candidates: Vec<usize> = graph.nodes.iter().enumerate().filter(|(_, n)| matches!(n.kind, NodeKind::Function | NodeKind::Method)).map(|(i, _)| i).collect();

    let m = candidates.len();
    let mut phi_flat = vec![0.0f32; m * kind_count];
    for (ci, &node_idx) in candidates.iter().enumerate() {
        let denom = (csr.row_ptr[node_idx + 1] - csr.row_ptr[node_idx]) as f32 + 1.0;
        let base = node_idx * kind_count;
        for k in 0..kind_count {
            phi_flat[ci * kind_count + k] = counts[base + k] as f32 / denom;
        }
    }

    let dist_matrix = cosine_distance_gpu(&phi_flat, m, kind_count);

    let ancestors = reachability_batched_gpu(&rev_csr, &candidates);

    let mut pairs = Vec::new();
    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let dist = dist_matrix[i * m + j];
            if dist < epsilon {
                let a = candidates[i];
                let b = candidates[j];
                let reachable = ancestors[i].iter().zip(ancestors[j].iter()).any(|(x, y)| *x && *y);
                pairs.push(DuplicatePair { left: graph.nodes[a].id, right: graph.nodes[b].id, distance: dist, reachable });
            }
        }
    }

    Ok(DuplicateReport { epsilon, pairs })
}

fn edge_kind_index(graph: &AnalysisGraph, kind: EdgeKind) -> Option<usize> {
    graph.edge_kinds.iter().position(|k| *k == kind)
}

fn build_id_index(graph: &AnalysisGraph) -> Result<Vec<usize>> {
    let max_id = graph.nodes.iter().map(|n| n.id as usize).max().unwrap_or(0);
    let mut id_to_index = vec![usize::MAX; max_id + 1];
    for (idx, node) in graph.nodes.iter().enumerate() {
        let slot = &mut id_to_index[node.id as usize];
        if *slot != usize::MAX {
            return Err(anyhow!("duplicate node id {}", node.id));
        }
        *slot = idx;
    }
    Ok(id_to_index)
}

fn map_id(id_to_index: &[usize], id: u32) -> Option<usize> {
    let idx = id as usize;
    if idx >= id_to_index.len() || id_to_index[idx] == usize::MAX {
        return None;
    }
    Some(id_to_index[idx])
}

fn build_csr_with_kinds(graph: &AnalysisGraph) -> Result<(Csr, Vec<u8>)> {
    let id_to_index = build_id_index(graph)?;
    let mut adj: Vec<Vec<(usize, u8)>> = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        let idx = edge_kind_index(graph, e.kind).ok_or_else(|| anyhow!("missing edge kind index"))?;
        let src = map_id(&id_to_index, e.src);
        let dst = map_id(&id_to_index, e.dst);
        if let (Some(src), Some(dst)) = (src, dst) {
            adj[src].push((dst, idx as u8));
        }
    }
    let mut row_ptr = Vec::with_capacity(adj.len() + 1);
    let mut col_idx = Vec::new();
    let mut edge_kind = Vec::new();
    row_ptr.push(0i32);
    for neighbours in adj {
        for (dst, kind) in neighbours {
            col_idx.push(dst as i32);
            edge_kind.push(kind);
        }
        row_ptr.push(col_idx.len() as i32);
    }
    Ok((Csr { row_ptr, col_idx }, edge_kind))
}

fn build_kind_csr(graph: &AnalysisGraph, kind: EdgeKind) -> Csr {
    let id_to_index = build_id_index(graph).expect("invalid node ids");
    let mut adj = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        if e.kind == kind {
            let src = map_id(&id_to_index, e.src);
            let dst = map_id(&id_to_index, e.dst);
            if let (Some(src), Some(dst)) = (src, dst) {
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
