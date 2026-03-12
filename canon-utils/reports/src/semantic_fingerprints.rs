use crate::artifacts_loader::KernelGraph;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct FingerprintSummary {
    pub kind: String,
    pub has_block: usize,
    pub has_param: usize,
    pub has_return: usize,
    pub callsite_calls: usize,
    pub blocks_flow: usize,
    pub blocks_unwind: usize,
}

#[allow(dead_code)]
pub fn compute_fingerprints(graph: &KernelGraph) -> Vec<FingerprintSummary> {
    let id_to_kind: HashMap<u32, &str> =
        graph.nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = Vec::new();

    let mut counts: HashMap<&str, FingerprintSummary> = HashMap::new();
    for e in &graph.edges {
        let src_kind = id_to_kind.get(&e.src).copied().unwrap_or("UNKNOWN");
        let entry = counts.entry(src_kind).or_insert(FingerprintSummary {
            kind: src_kind.to_string(),
            has_block: 0,
            has_param: 0,
            has_return: 0,
            callsite_calls: 0,
            blocks_flow: 0,
            blocks_unwind: 0,
        });
        match e.kind.as_str() {
            "HAS_BLOCK" => entry.has_block += 1,
            "HAS_PARAM" => entry.has_param += 1,
            "RETURN" => entry.has_return += 1,
            "CALL" => entry.callsite_calls += 1,
            "FLOW" => entry.blocks_flow += 1,
            "UNWIND" => entry.blocks_unwind += 1,
            _ => {}
        }
    }
    for v in counts.into_values() {
        out.push(v);
    }
    out
}
