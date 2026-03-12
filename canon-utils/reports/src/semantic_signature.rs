use crate::semantic_features::NodeFeatureVector;
use anyhow::Result;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SemanticSignature {
    pub node_id: u32,
    pub signature: u64,
}

pub fn compute_signatures(graph_dir: &Path, feats: &[NodeFeatureVector]) -> Result<Vec<SemanticSignature>> {
    let mut out = Vec::with_capacity(feats.len());
    for f in feats {
        let sig = signature_for(f);
        out.push(SemanticSignature { node_id: f.node_id, signature: sig });
    }
    write_signatures_csv(graph_dir, &out)?;
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

fn write_signatures_csv(graph_dir: &Path, sigs: &[SemanticSignature]) -> Result<()> {
    let path = graph_dir
        .parent()
        .unwrap_or(graph_dir)
        .join("semantics")
        .join("node_semantic_signatures.csv");
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
