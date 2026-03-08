use crate::loader::{AnalysisGraph, EdgeKind};
use algorithms::graph::bellman_ford_gpu::bellman_ford_gpu;
use algorithms::graph::csr::Csr;
use algorithms::graph::depth_gpu::longest_path_depth_gpu;
use algorithms::graph::topological_sort_gpu::topological_sort_gpu;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct AnomalyReport {
    pub depth_outliers: Vec<u32>,
    pub topo_missing: Vec<u32>,
    pub bellman_dist: Vec<u64>,
}

pub fn analyze_anomalies(graph: &AnalysisGraph) -> AnomalyReport {
    let flow = build_kind_csr(graph, EdgeKind::Flow);
    let depth = longest_path_depth_gpu(&flow);
    let (means, stds) = depth_stats(graph, &depth);
    let mut depth_outliers = Vec::new();
    for (idx, d) in depth.iter().enumerate() {
        let kind = graph.nodes[idx].kind;
        if let Some((mu, sigma)) = means.get(&kind).zip(stds.get(&kind)) {
            if *sigma > 0.0 && (*d as f64 - *mu).abs() > 3.0 * *sigma {
                depth_outliers.push(graph.nodes[idx].id);
            }
        }
    }

    let order = topological_sort_gpu(&flow);
    let mut seen = vec![false; flow.vertex_count()];
    for &v in &order {
        if v < seen.len() {
            seen[v] = true;
        }
    }
    let mut topo_missing = Vec::new();
    for (idx, ok) in seen.iter().enumerate() {
        if !*ok {
            topo_missing.push(graph.nodes[idx].id);
        }
    }

    let error_edges = edge_list_with_weights(graph, EdgeKind::ErrorToFunction);
    let bellman_dist = bellman_ford_gpu(graph.nodes.len(), &error_edges, 0).unwrap_or_default();

    AnomalyReport {
        depth_outliers,
        topo_missing,
        bellman_dist,
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

fn depth_stats(graph: &AnalysisGraph, depth: &[i32]) -> (BTreeMap<crate::loader::NodeKind, f64>, BTreeMap<crate::loader::NodeKind, f64>) {
    let mut sums: BTreeMap<crate::loader::NodeKind, f64> = BTreeMap::new();
    let mut counts: BTreeMap<crate::loader::NodeKind, f64> = BTreeMap::new();
    for (idx, d) in depth.iter().enumerate() {
        let kind = graph.nodes[idx].kind;
        *sums.entry(kind).or_insert(0.0) += *d as f64;
        *counts.entry(kind).or_insert(0.0) += 1.0;
    }
    let mut means = BTreeMap::new();
    for (k, sum) in &sums {
        let c = counts.get(k).copied().unwrap_or(1.0);
        means.insert(*k, sum / c);
    }
    let mut var = BTreeMap::new();
    for (idx, d) in depth.iter().enumerate() {
        let kind = graph.nodes[idx].kind;
        let mu = means.get(&kind).copied().unwrap_or(0.0);
        let v = (*d as f64 - mu) * (*d as f64 - mu);
        *var.entry(kind).or_insert(0.0) += v;
    }
    let mut stds = BTreeMap::new();
    for (k, v) in &var {
        let c = counts.get(k).copied().unwrap_or(1.0);
        stds.insert(*k, (v / c).sqrt());
    }
    (means, stds)
}

fn edge_list_with_weights(graph: &AnalysisGraph, kind: EdgeKind) -> Vec<(usize, usize, u64)> {
    let mut edges = Vec::new();
    let rank = error_rank(graph);
    for e in &graph.edges {
        if e.kind == kind {
            let w = rank.get(&e.dst).copied().unwrap_or(1);
            edges.push((e.src as usize, e.dst as usize, w));
        }
    }
    edges
}

fn error_rank(graph: &AnalysisGraph) -> BTreeMap<u32, u64> {
    let mut rank = BTreeMap::new();
    for e in &graph.edges {
        if e.kind == EdgeKind::ErrorToFunction {
            *rank.entry(e.dst).or_insert(0) += 1;
        }
    }
    rank
}
