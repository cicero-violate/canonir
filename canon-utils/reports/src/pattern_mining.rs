use crate::semantic_clustering::SemanticCluster;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PatternRule {
    pub rule: String,
    pub support: f64,
    pub confidence: f64,
}

pub fn mine_patterns(clusters: &[SemanticCluster]) -> Vec<PatternRule> {
    let mut out = Vec::new();
    for c in clusters {
        if c.nodes.len() < 2 {
            continue;
        }
        let rule = format!("cluster_{}: kind={}", c.cluster_id, c.node_kind);
        out.push(PatternRule {
            rule,
            support: 1.0,
            confidence: 1.0,
        });
    }
    out
}
