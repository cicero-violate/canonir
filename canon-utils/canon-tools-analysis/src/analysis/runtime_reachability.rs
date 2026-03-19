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
    let entry_match = entry_override.as_deref();

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
    let total_functions = fn_nodes.len();

    let adj = build_callgraph_adj(callgraph);
    let (entry_node_id, entry_symbol, note) = if let Some(sym) = entry_match {
        match find_entry_node(node_map, sym) {
            Some(id) => (Some(id), sym.to_string(), None),
            None => {
                let msg = format!("runtime entry symbol not found: {sym}");
                (None, sym.to_string(), Some(msg))
            }
        }
    } else {
        // Default to the canon-runtime event runtime binary main symbol, by file path.
        match find_entry_node_by_file_main(node_map, file_map, "canon-runtime/src/bin/event_runtime.rs") {
            Some((id, sym)) => (Some(id), sym, None),
            None => {
                let msg = "runtime entry symbol not found: set CANON_RUNTIME_ENTRY_SYMBOL to a concrete symbol".to_string();
                (None, "main".to_string(), Some(msg))
            }
        }
    };

    if entry_node_id.is_none() {
        return Ok(RuntimeReachabilityReport {
            entry_symbol,
            entry_node_id: None,
            total_functions,
            reachable_functions: 0,
            coverage_ratio: 0.0,
            unreachable: Vec::new(),
            note,
        });
    }
    let entry_node_id = entry_node_id.expect("entry_node_id guarded above");

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

    let reachable_functions = reachable.len();
    let coverage_ratio = if total_functions == 0 {
        0.0
    } else {
        reachable_functions as f64 / total_functions as f64
    };

    Ok(RuntimeReachabilityReport {
        entry_symbol,
        entry_node_id: Some(entry_node_id),
        total_functions,
        reachable_functions,
        coverage_ratio,
        unreachable,
        note,
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

fn find_entry_node_by_file_main(
    node_map: &HashMap<u32, CodeGraphNode>,
    file_map: &HashMap<u32, String>,
    file_suffix: &str,
) -> Option<(u32, String)> {
    let mut found: Option<(u32, String)> = None;
    for (id, node) in node_map {
        if node.kind != "FUNCTION" && node.kind != "METHOD" {
            continue;
        }
        let sym = node.symbol.as_str();
        if sym != "main" && !sym.starts_with("main#") {
            continue;
        }
        let file = node
            .file_id
            .and_then(|fid| file_map.get(&fid))
            .map(|s| s.as_str())
            .unwrap_or("");
        if !file.ends_with(file_suffix) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some((*id, sym.to_string()));
    }
    found
}
