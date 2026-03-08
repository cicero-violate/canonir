use crate::loader::{AnalysisGraph, EdgeKind, NodeKind};
use anyhow::{anyhow, Result};
use algorithms::graph::csr::Csr;
use algorithms::graph::feature_gpu::edge_kind_histogram_gpu;
use algorithms::graph::reachability::reachability_gpu;
use serde::Serialize;

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
    let mut phi: Vec<Vec<f32>> = Vec::with_capacity(graph.nodes.len());
    for idx in 0..graph.nodes.len() {
        let denom = (csr.row_ptr[idx + 1] - csr.row_ptr[idx]) as f32 + 1.0;
        let mut v = vec![0.0f32; kind_count];
        let base = idx * kind_count;
        for k in 0..kind_count {
            v[k] = counts[base + k] as f32 / denom;
        }
        phi.push(v);
    }

    let call_csr = build_kind_csr(graph, EdgeKind::Call);
    let candidates: Vec<usize> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.kind, NodeKind::Function | NodeKind::Method))
        .map(|(i, _)| i)
        .collect();

    let mut pairs = Vec::new();
    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let a = candidates[i];
            let b = candidates[j];
            let dist = cosine_distance(&phi[a], &phi[b]);
            if dist < epsilon {
                let reachable = share_common_ancestor(&call_csr, a, b);
                pairs.push(DuplicatePair {
                    left: graph.nodes[a].id,
                    right: graph.nodes[b].id,
                    distance: dist,
                    reachable,
                });
            }
        }
    }

    Ok(DuplicateReport { epsilon, pairs })
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na.sqrt() * nb.sqrt()).max(1e-6);
    1.0 - dot / denom
}

fn edge_kind_index(graph: &AnalysisGraph, kind: EdgeKind) -> Option<usize> {
    graph.edge_kinds.iter().position(|k| *k == kind)
}

fn build_csr_with_kinds(graph: &AnalysisGraph) -> Result<(Csr, Vec<u8>)> {
    let mut adj: Vec<Vec<(usize, u8)>> = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        let idx = edge_kind_index(graph, e.kind).ok_or_else(|| anyhow!("missing edge kind index"))?;
        adj[e.src as usize].push((e.dst as usize, idx as u8));
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
    let mut adj = vec![Vec::new(); graph.nodes.len()];
    for e in &graph.edges {
        if e.kind == kind {
            adj[e.src as usize].push(e.dst as usize);
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

fn share_common_ancestor(call_csr: &Csr, a: usize, b: usize) -> bool {
    let rev = reverse_csr(call_csr);
    let ancestors_a = reachability_gpu(&rev, &[a]);
    let ancestors_b = reachability_gpu(&rev, &[b]);
    for idx in 0..ancestors_a.len() {
        if ancestors_a[idx] && ancestors_b[idx] {
            return true;
        }
    }
    false
}
