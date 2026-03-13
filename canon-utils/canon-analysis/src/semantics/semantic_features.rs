use canon_graph::artifacts_loader::KernelGraph;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const EDGE_KIND_COUNT: usize = 22;

#[derive(Debug, Clone)]
pub struct NodeFeatureVector {
    pub node_id: u32,
    pub node_kind: u8,
    pub indegree: u32,
    pub outdegree: u32,
    pub edge_histogram: [u32; EDGE_KIND_COUNT],
    pub neighbor_kind_histogram: [u32; 16],
}

pub fn extract_node_features(graph_dir: &Path, graph: &KernelGraph) -> Result<Vec<NodeFeatureVector>> {
    let mut indeg: HashMap<u32, u32> = HashMap::new();
    let mut outdeg: HashMap<u32, u32> = HashMap::new();
    let mut edge_hist: HashMap<u32, [u32; EDGE_KIND_COUNT]> = HashMap::new();
    let mut neighbor_hist: HashMap<u32, [u32; 16]> = HashMap::new();
    let id_to_kind: HashMap<u32, u8> = graph
        .nodes
        .iter()
        .map(|n| (n.id, node_kind_code(&n.kind)))
        .collect();

    for e in &graph.edges {
        *outdeg.entry(e.src).or_default() += 1;
        *indeg.entry(e.dst).or_default() += 1;
        let idx = edge_kind_index(&e.kind);
        if let Some(h) = edge_hist.get_mut(&e.src) {
            if idx < EDGE_KIND_COUNT {
                h[idx] += 1;
            }
        } else {
            let mut h = [0u32; EDGE_KIND_COUNT];
            if idx < EDGE_KIND_COUNT {
                h[idx] = 1;
            }
            edge_hist.insert(e.src, h);
        }
        let dst_kind = id_to_kind.get(&e.dst).copied().unwrap_or(0);
        let entry = neighbor_hist.entry(e.src).or_insert([0u32; 16]);
        let k = dst_kind as usize;
        if k < entry.len() {
            entry[k] += 1;
        }
    }

    let mut out = Vec::with_capacity(graph.nodes.len());
    for n in &graph.nodes {
        let mut eh = [0u32; EDGE_KIND_COUNT];
        if let Some(h) = edge_hist.get(&n.id) {
            eh = *h;
        }
        let mut nh = [0u32; 16];
        if let Some(h) = neighbor_hist.get(&n.id) {
            nh = *h;
        }
        out.push(NodeFeatureVector {
            node_id: n.id,
            node_kind: node_kind_code(&n.kind),
            indegree: *indeg.get(&n.id).unwrap_or(&0),
            outdegree: *outdeg.get(&n.id).unwrap_or(&0),
            edge_histogram: eh,
            neighbor_kind_histogram: nh,
        });
    }

    write_node_features_bin(graph_dir, &out)?;
    Ok(out)
}

fn write_node_features_bin(graph_dir: &Path, feats: &[NodeFeatureVector]) -> Result<()> {
    let path = graph_dir.join("node_features.bin");
    let mut buf = Vec::with_capacity(4 + feats.len() * 64);
    buf.extend_from_slice(&(feats.len() as u32).to_le_bytes());
    for f in feats {
        buf.extend_from_slice(&f.node_id.to_le_bytes());
        buf.push(f.node_kind);
        buf.extend_from_slice(&f.indegree.to_le_bytes());
        buf.extend_from_slice(&f.outdegree.to_le_bytes());
        for v in f.edge_histogram.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for v in f.neighbor_kind_histogram.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    fs::write(path, buf)?;
    Ok(())
}

fn node_kind_code(kind: &str) -> u8 {
    match kind {
        "FUNCTION" => 1,
        "METHOD" => 2,
        "STRUCT" => 3,
        "ENUM" => 4,
        "TRAIT" => 5,
        "IMPL" => 6,
        "FIELD" => 7,
        "PARAM" => 8,
        "VARIABLE" => 9,
        "MODULE" => 10,
        "TYPE" => 11,
        "BASIC_BLOCK" => 12,
        "CALL_SITE" => 13,
        _ => 0,
    }
}

fn edge_kind_index(kind: &str) -> usize {
    match kind {
        "CONTAINS" => 0,
        "HAS_FIELD" => 1,
        "HAS_METHOD" => 2,
        "HAS_BLOCK" => 3,
        "HAS_PARAM" => 4,
        "IMPORTS" => 5,
        "EXPORT" => 6,
        "PUBLIC_USE" => 7,
        "FLOW" => 8,
        "CALL" => 9,
        "RETURN" => 10,
        "UNWIND" => 11,
        "IMPLEMENTS" => 12,
        "FOR_TYPE" => 13,
        "USES_TYPE" => 14,
        "BOUNDS" => 15,
        "ASSIGN" => 16,
        "PROPAGATES" => 17,
        "ARG_TO_PARAM" => 18,
        "RETURNS" => 19,
        "ERROR_TO_FUNCTION" => 20,
        "ERROR_TO_BLOCK" => 21,
        _ => EDGE_KIND_COUNT,
    }
}
