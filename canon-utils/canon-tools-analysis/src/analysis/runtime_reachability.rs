use std::collections::{HashMap, HashSet, VecDeque};

use canon_graph::graph::graph_types::CodeGraphNode;

use crate::analysis::callgraph::build_callgraph_adj;
use crate::{RuntimeReachabilityEntry, RuntimeReachabilityReport};

pub fn build_runtime_reachability_report(
    node_map: &HashMap<u32, CodeGraphNode>,
    file_map: &HashMap<u32, String>,
    callgraph: &[(u32, u32)],
) -> anyhow::Result<RuntimeReachabilityReport> {
    let entry_override = std::env::var("CANON_RUNTIME_ENTRY_SYMBOL").ok();
    let entry_match = entry_override
        .as_deref()
        .unwrap_or("event_runtime::main");

    let adj = build_callgraph_adj(callgraph);
    let entry_node_id = find_entry_node(node_map, entry_match)
        .ok_or_else(|| anyhow::anyhow!("runtime entry symbol not found: {}", entry_match))?;

    let fn_nodes: Vec<u32> = node_map
        .iter()
        .filter_map(|(id, n)| {
            if n.kind == "FUNCTION" || n.kind == "METHOD" {
                Some(*id)
            } else {
                None
            }
        })
        .collect();

    let mut reachable = HashSet::new();
    let mut queue = VecDeque::from([entry_node_id]);
    while let Some(node) = queue.pop_front() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(nexts) = adj.get(&node) {
            for dst in nexts {
                queue.push_back(*dst);
            }
        }
    }

    let mut unreachable = Vec::new();
    for id in &fn_nodes {
        if reachable.contains(id) {
            continue;
        }
        let node = node_map.get(id);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|fid| file_map.get(&fid).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        unreachable.push(RuntimeReachabilityEntry { symbol, file, line });
    }

    let total_functions = fn_nodes.len();
    let reachable_functions = reachable.len();
    let coverage_ratio = if total_functions == 0 {
        0.0
    } else {
        reachable_functions as f64 / total_functions as f64
    };

    Ok(RuntimeReachabilityReport {
        entry_symbol: entry_match.to_string(),
        entry_node_id: Some(entry_node_id),
        total_functions,
        reachable_functions,
        coverage_ratio,
        unreachable,
        note: None,
    })
}

fn find_entry_node(
    node_map: &HashMap<u32, CodeGraphNode>,
    entry_match: &str,
) -> Option<u32> {
    for (id, node) in node_map {
        if node.kind != "FUNCTION" && node.kind != "METHOD" {
            continue;
        }
        let sym = node.symbol.as_str();
        if sym == entry_match {
            return Some(*id);
        }
    }
    None
}
