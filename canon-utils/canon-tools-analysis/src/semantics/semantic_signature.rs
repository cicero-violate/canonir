use crate::analysis::callgraph::find_callgraph_roots;
use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode};
use crate::semantics::semantic_features::NodeFeatureVector;
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SemanticSignature {
    pub node_id: u32,
    pub signature: u64,
}

pub fn compute_signatures(metrics_dir: &Path, feats: &[NodeFeatureVector]) -> Result<Vec<SemanticSignature>> {
    let mut out = Vec::with_capacity(feats.len());
    for f in feats {
        let sig = signature_for(f);
        out.push(SemanticSignature { node_id: f.node_id, signature: sig });
    }
    write_signatures_csv(metrics_dir, &out)?;
    Ok(out)
}

fn signature_for(f: &NodeFeatureVector) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    f.node_kind.hash(&mut hasher);
    f.indegree.hash(&mut hasher);
    f.outdegree.hash(&mut hasher);
    f.edge_histogram.hash(&mut hasher);
    f.neighbor_kind_histogram.hash(&mut hasher);
    hasher.finish()
}

fn write_signatures_csv(metrics_dir: &Path, sigs: &[SemanticSignature]) -> Result<()> {
    let path = metrics_dir.join("semantic_signatures.csv");
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut buf = String::with_capacity(sigs.len() * 32 + 64);
    buf.push_str("node_id,signature\n");
    for s in sigs {
        buf.push_str(&format!("{},{}\n", s.node_id, s.signature));
    }
    fs::write(path, buf)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct SemanticSignatureReport {
    node_id: u32,
    symbol: String,
    node_type: String,
    fan_in: u32,
    fan_out: u32,
    call_depth: u32,
    mutation_rate: f64,
}

pub fn write_semantic_signatures(
    _graph_dir: &Path,
    reports_dir: &Path,
    nodes: &[CodeGraphNode],
    edges: &[CodeGraphEdge],
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
    let mut out: Vec<SemanticSignatureReport> = Vec::with_capacity(nodes.len());
    for n in nodes {
        out.push(SemanticSignatureReport {
            node_id: n.id,
            symbol: n.symbol.clone(),
            node_type: n.kind.clone(),
            fan_in: *fan_in.get(&n.id).unwrap_or(&0),
            fan_out: *fan_out.get(&n.id).unwrap_or(&0),
            call_depth: *call_depth.get(&n.id).unwrap_or(&0),
            mutation_rate: 0.0,
        });
    }
    fs::write(
        reports_dir.join("semantic_signatures.json"),
        serde_json::to_string_pretty(&out)?,
    )?;
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
