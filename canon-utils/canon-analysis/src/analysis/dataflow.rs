use std::collections::{HashMap, HashSet};

use canon_graph::graph::graph_types::{EdgeRow, NodeRow};
use crate::DataflowFanoutEntry;

pub fn build_dataflow_fanout(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
    block_owner: &HashMap<u32, u32>,
) -> Vec<DataflowFanoutEntry> {
    let mut out = Vec::new();
    let mut fn_nodes: Vec<u32> = Vec::new();
    for n in nodes {
        if n.kind == "FUNCTION" || n.kind == "METHOD" {
            fn_nodes.push(n.id);
        }
    }

    let mutation_kinds: HashSet<&str> = ["ASSIGN", "PROPAGATES", "ARG_TO_PARAM", "RETURNS"].into_iter().collect();
    let io_kinds: HashSet<&str> = ["CALL", "RETURN"].into_iter().collect();

    let mut edges_by_fn: HashMap<u32, Vec<&EdgeRow>> = HashMap::new();
    for e in edges {
        let owner = block_owner.get(&e.src).copied().or_else(|| block_owner.get(&e.dst).copied());
        if let Some(fid) = owner {
            edges_by_fn.entry(fid).or_default().push(e);
        }
    }

    for fid in fn_nodes {
        let node = node_map.get(&fid);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        let fn_edges = edges_by_fn.get(&fid).cloned().unwrap_or_default();
        let outgoing_edges = fn_edges.len();
        let mutation_edges = fn_edges.iter().filter(|e| mutation_kinds.contains(e.kind.as_str())).count();
        let io_edges = fn_edges.iter().filter(|e| io_kinds.contains(e.kind.as_str())).count();
        out.push(DataflowFanoutEntry { symbol, file, line, outgoing_edges, mutation_edges, io_edges });
    }
    out.sort_by(|a, b| b.outgoing_edges.cmp(&a.outgoing_edges));
    out
}
