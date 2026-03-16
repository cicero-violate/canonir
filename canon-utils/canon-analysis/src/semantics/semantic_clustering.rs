use crate::analysis::callgraph::find_callgraph_roots;
use canon_graph::graph::graph_types::{CodeEdge, CodeNode};
use crate::semantics::semantic_features::NodeFeatureVector;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize)]
struct SemanticClusterReport {
    cluster_id: u32,
    label: String,
    size: usize,
    avg_fan_in: f64,
    avg_fan_out: f64,
    avg_call_depth: f64,
    nodes: Vec<u32>,
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

pub fn write_semantic_clusters(
    _graph_dir: &Path,
    reports_dir: &Path,
    nodes: &[CodeNode],
    edges: &[CodeEdge],
    callgraph: &[(u32, u32)],
) -> Result<()> {
    fs::create_dir_all(reports_dir)?;
    let mut fan_in: HashMap<u32, u32> = HashMap::new();
    let mut fan_out: HashMap<u32, u32> = HashMap::new();
    for e in edges {
        *fan_out.entry(e.src).or_insert(0) += 1;
        *fan_in.entry(e.dst).or_insert(0) += 1;
    }
    let call_depth = compute_call_depths(callgraph);
    let mut points: Vec<(u32, [f64; 3])> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let fi = *fan_in.get(&n.id).unwrap_or(&0) as f64;
        let fo = *fan_out.get(&n.id).unwrap_or(&0) as f64;
        let cd = *call_depth.get(&n.id).unwrap_or(&0) as f64;
        points.push((n.id, [fi, fo, cd]));
    }
    let k = 4usize.min(points.len().max(1));
    let mut centroids: Vec<[f64; 3]> = Vec::new();
    for (idx, (_id, feat)) in points.iter().enumerate() {
        if centroids.len() >= k {
            break;
        }
        if idx % (points.len().max(1) / k.max(1)).max(1) == 0 {
            centroids.push(*feat);
        }
    }
    if centroids.is_empty() {
        centroids.push([0.0, 0.0, 0.0]);
    }
    let mut assignments: Vec<usize> = vec![0; points.len()];
    for _ in 0..8 {
        for (i, (_id, feat)) in points.iter().enumerate() {
            let mut best = 0;
            let mut best_dist = f64::MAX;
            for (c_idx, c) in centroids.iter().enumerate() {
                let dist = (feat[0] - c[0]).powi(2)
                    + (feat[1] - c[1]).powi(2)
                    + (feat[2] - c[2]).powi(2);
                if dist < best_dist {
                    best_dist = dist;
                    best = c_idx;
                }
            }
            assignments[i] = best;
        }
        let mut sums = vec![[0.0, 0.0, 0.0]; centroids.len()];
        let mut counts = vec![0u32; centroids.len()];
        for (i, (_id, feat)) in points.iter().enumerate() {
            let idx = assignments[i];
            sums[idx][0] += feat[0];
            sums[idx][1] += feat[1];
            sums[idx][2] += feat[2];
            counts[idx] += 1;
        }
        for i in 0..centroids.len() {
            let c = counts[i].max(1) as f64;
            centroids[i] = [sums[i][0] / c, sums[i][1] / c, sums[i][2] / c];
        }
    }

    let mut clusters: Vec<SemanticClusterReport> = Vec::new();
    for (idx, centroid) in centroids.iter().enumerate() {
        let mut nodes_in = Vec::new();
        for (i, (id, _feat)) in points.iter().enumerate() {
            if assignments[i] == idx {
                nodes_in.push(*id);
            }
        }
        let label = label_cluster(centroid[0], centroid[1], centroid[2]);
        clusters.push(SemanticClusterReport {
            cluster_id: idx as u32,
            label,
            size: nodes_in.len(),
            avg_fan_in: centroid[0],
            avg_fan_out: centroid[1],
            avg_call_depth: centroid[2],
            nodes: nodes_in,
        });
    }

    fs::write(
        reports_dir.join("semantic_clusters.json"),
        serde_json::to_string_pretty(&clusters)?,
    )?;

    write_cluster_graph_bin(&reports_dir.join("cluster_graph.bin"), &clusters, edges)?;

    Ok(())
}

fn compute_call_depths(callgraph: &[(u32, u32)]) -> HashMap<u32, u32> {
    let roots = find_callgraph_roots(callgraph);
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (s, d) in callgraph {
        adj.entry(*s).or_default().push(*d);
    }
    let mut depth: HashMap<u32, u32> = HashMap::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    for r in roots {
        depth.insert(r, 0);
        queue.push_back(r);
    }
    while let Some(v) = queue.pop_front() {
        let next_depth = depth.get(&v).copied().unwrap_or(0).saturating_add(1);
        if let Some(neigh) = adj.get(&v) {
            for n in neigh {
                if !depth.contains_key(n) {
                    depth.insert(*n, next_depth);
                    queue.push_back(*n);
                }
            }
        }
    }
    depth
}

fn label_cluster(fan_in: f64, fan_out: f64, call_depth: f64) -> String {
    if fan_out >= fan_in * 1.5 && call_depth >= 2.0 {
        return "orchestration".to_string();
    }
    if fan_in >= fan_out * 1.5 {
        return "state".to_string();
    }
    if call_depth <= 1.0 {
        return "compute".to_string();
    }
    "io".to_string()
}

fn write_cluster_graph_bin(
    path: &Path,
    clusters: &[SemanticClusterReport],
    edges: &[CodeEdge],
) -> Result<()> {
    const MAGIC: &[u8; 4] = b"CCGB";
    const VERSION: u32 = 1;
    let mut node_to_cluster: HashMap<u32, u32> = HashMap::new();
    for c in clusters {
        for id in &c.nodes {
            node_to_cluster.insert(*id, c.cluster_id);
        }
    }
    let mut edge_counts: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    for e in edges {
        let Some(src_c) = node_to_cluster.get(&e.src).copied() else { continue; };
        let Some(dst_c) = node_to_cluster.get(&e.dst).copied() else { continue; };
        if src_c == dst_c {
            continue;
        }
        *edge_counts.entry((src_c, dst_c)).or_insert(0) += 1;
    }
    let n_clusters = clusters.len() as u32;
    let n_edges = edge_counts.len() as u32;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&n_clusters.to_le_bytes());
    out.extend_from_slice(&n_edges.to_le_bytes());
    for c in clusters {
        out.extend_from_slice(&c.cluster_id.to_le_bytes());
        out.extend_from_slice(&c.size.to_le_bytes());
        out.extend_from_slice(&c.avg_fan_in.to_le_bytes());
        out.extend_from_slice(&c.avg_fan_out.to_le_bytes());
        out.extend_from_slice(&c.avg_call_depth.to_le_bytes());
    }
    for ((src, dst), count) in edge_counts {
        out.extend_from_slice(&src.to_le_bytes());
        out.extend_from_slice(&dst.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
    }
    fs::write(path, out)?;
    Ok(())
}
