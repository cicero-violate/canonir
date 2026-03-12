use crate::semantic_features::NodeFeatureVector;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize)]
pub struct SemanticCluster {
    pub cluster_id: u64,
    pub node_kind: u8,
    pub nodes: Vec<u32>,
}

pub struct ClusteringResult {
    pub clusters: Vec<SemanticCluster>,
    pub outliers: Vec<u32>,
}

pub fn cluster_dbscan_like(
    feats: &[NodeFeatureVector],
    eps: f64,
    min_pts: usize,
) -> ClusteringResult {
    let mut by_kind: HashMap<u8, Vec<&NodeFeatureVector>> = HashMap::new();
    for f in feats {
        by_kind.entry(f.node_kind).or_default().push(f);
    }

    let mut clusters = Vec::new();
    let mut outliers = Vec::new();
    let mut cluster_id = 0u64;

    for (kind, items) in by_kind {
        let mut visited: HashSet<u32> = HashSet::new();
        for f in &items {
            if visited.contains(&f.node_id) {
                continue;
            }
            visited.insert(f.node_id);
            let neighbors = region_query(f, &items, eps);
            if neighbors.len() + 1 < min_pts {
                outliers.push(f.node_id);
                continue;
            }
            cluster_id += 1;
            let mut cluster_nodes = vec![f.node_id];
            let mut seed = neighbors;
            let mut i = 0usize;
            while i < seed.len() {
                let nid = seed[i];
                i += 1;
                if !visited.insert(nid) {
                    continue;
                }
                let nf = items.iter().find(|v| v.node_id == nid).unwrap();
                let neigh = region_query(nf, &items, eps);
                if neigh.len() + 1 >= min_pts {
                    for n in neigh {
                        if !seed.contains(&n) {
                            seed.push(n);
                        }
                    }
                }
                cluster_nodes.push(nid);
            }
            cluster_nodes.sort_unstable();
            clusters.push(SemanticCluster {
                cluster_id,
                node_kind: kind,
                nodes: cluster_nodes,
            });
        }
    }

    outliers.sort_unstable();
    clusters.sort_by(|a, b| {
        a.node_kind
            .cmp(&b.node_kind)
            .then_with(|| a.nodes.len().cmp(&b.nodes.len()))
            .then_with(|| a.nodes.first().cmp(&b.nodes.first()))
    });
    for (idx, cluster) in clusters.iter_mut().enumerate() {
        cluster.cluster_id = idx as u64;
    }

    ClusteringResult { clusters, outliers }
}

fn region_query(
    f: &NodeFeatureVector,
    items: &[&NodeFeatureVector],
    eps: f64,
) -> Vec<u32> {
    let mut out = Vec::new();
    for other in items {
        if f.node_id == other.node_id {
            continue;
        }
        if distance(f, other) <= eps {
            out.push(other.node_id);
        }
    }
    out
}

fn distance(a: &NodeFeatureVector, b: &NodeFeatureVector) -> f64 {
    let mut d = 0f64;
    d += (a.indegree as f64 - b.indegree as f64).abs();
    d += (a.outdegree as f64 - b.outdegree as f64).abs();
    for (x, y) in a.edge_histogram.iter().zip(b.edge_histogram.iter()) {
        d += (*x as f64 - *y as f64).abs();
    }
    for (x, y) in a.neighbor_kind_histogram.iter().zip(b.neighbor_kind_histogram.iter()) {
        d += (*x as f64 - *y as f64).abs();
    }
    d
}
