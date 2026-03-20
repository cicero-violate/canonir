use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::CallgraphCentralityEntry;
use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode};

pub fn extract_callgraph_edges(nodes: &[CodeGraphNode], edges: &[CodeGraphEdge]) -> Vec<(u32, u32)> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut callsite_to_block: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut block_to_fn: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut has_callsite_edges = false;

    for edge in edges {
        if edge.kind != "HAS_BLOCK" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") && dst_kind == Some(&"CALL_SITE") {
            callsite_to_block.entry(edge.dst).or_default().insert(edge.src);
            has_callsite_edges = true;
        } else if matches!(src_kind, Some(&"FUNCTION" | &"METHOD")) && dst_kind == Some(&"BASIC_BLOCK") {
            block_to_fn.entry(edge.dst).or_default().insert(edge.src);
        }
    }

    for edge in edges {
        if edge.kind != "CALL" {
            continue;
        }
        let callee_kind = id_to_kind.get(&edge.dst);
        if !matches!(callee_kind, Some(&"FUNCTION" | &"METHOD")) {
            continue;
        }
        if has_callsite_edges {
            if let Some(blocks) = callsite_to_block.get(&edge.src) {
                for block in blocks {
                    if let Some(callers) = block_to_fn.get(block) {
                        for caller in callers {
                            seen.insert((*caller, edge.dst));
                        }
                    }
                }
            }
        } else {
            let caller_kind = id_to_kind.get(&edge.src);
            if matches!(caller_kind, Some(&"FUNCTION" | &"METHOD")) {
                seen.insert((edge.src, edge.dst));
            }
        }
    }

    seen.into_iter().collect()
}

pub fn build_callgraph_adj(callgraph: &[(u32, u32)]) -> HashMap<u32, Vec<u32>> {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (src, dst) in callgraph {
        adj.entry(*src).or_default().push(*dst);
    }
    adj
}

pub fn dfs_callgraph(adj: &HashMap<u32, Vec<u32>>, start: u32, visited: &mut HashSet<u32>) {
    if !visited.insert(start) {
        return;
    }
    if let Some(nexts) = adj.get(&start) {
        for dst in nexts {
            dfs_callgraph(adj, *dst, visited);
        }
    }
}

pub fn find_callgraph_roots(callgraph: &[(u32, u32)]) -> Vec<u32> {
    let mut incoming = HashSet::new();
    let mut nodes = HashSet::new();
    for (src, dst) in callgraph {
        nodes.insert(*src);
        nodes.insert(*dst);
        incoming.insert(*dst);
    }
    nodes.into_iter().filter(|n| !incoming.contains(n)).collect()
}

pub fn find_callgraph_roots_from_edges(cg_local_to_id: &[u32]) -> Vec<u32> {
    cg_local_to_id.iter().copied().collect()
}

pub fn build_callgraph_centrality(callgraph: &[(u32, u32)], node_map: &HashMap<u32, CodeGraphNode>, file_map: &HashMap<u32, String>) -> Vec<CallgraphCentralityEntry> {
    let mut callers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    let mut callees: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for (s, d) in callgraph {
        callers.entry(*d).or_default().insert(*s);
        callees.entry(*s).or_default().insert(*d);
    }
    let mut out = Vec::new();
    let mut node_ids: BTreeSet<u32> = BTreeSet::new();
    for (s, d) in callgraph {
        node_ids.insert(*s);
        node_ids.insert(*d);
    }
    for id in node_ids {
        let node = node_map.get(&id);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node.and_then(|n| n.file_id).and_then(|id| file_map.get(&id).cloned()).unwrap_or_default();
        let caller_count = callers.get(&id).map(|s| s.len()).unwrap_or(0);
        let callee_count = callees.get(&id).map(|s| s.len()).unwrap_or(0);
        let centrality_score = caller_count + callee_count;
        out.push(CallgraphCentralityEntry { symbol, file, caller_count, callee_count, centrality_score });
    }
    out.sort_by(|a, b| b.centrality_score.cmp(&a.centrality_score));
    out
}
