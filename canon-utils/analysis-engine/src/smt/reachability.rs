use crate::loader::{AnalysisGraph, EdgeKind, NodeKind};
use crate::smt::encoder::EncodedGraph;
use crate::smt::cache::{reachability_key, function_graph_hash, CacheEntry, now_ts};
use algorithms::graph::csr::Csr;
use algorithms::graph::reachability::reachability_gpu;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use z3::SatResult;

#[derive(Debug, Serialize)]
pub struct SmtReachabilityEntry {
    pub node_id: u32,
    pub smt_reachable: bool,
    pub smt_path_condition: BTreeMap<String, bool>,
}

pub fn check_repair_surface(
    session: &crate::smt::SmtSession,
    graph: &AnalysisGraph,
    repair_surface: &Value,
) -> Vec<SmtReachabilityEntry> {
    let mut out = Vec::new();
    let top_k = repair_surface
        .get("top_k")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for entry in top_k {
        let node_id = entry.get("node_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if node_id == 0 {
            continue;
        }
        let (reachable, path) = check_function_errors(session, graph, node_id);
        out.push(SmtReachabilityEntry {
            node_id,
            smt_reachable: reachable,
            smt_path_condition: path,
        });
    }
    out
}

fn check_function_errors(
    session: &crate::smt::SmtSession,
    graph: &AnalysisGraph,
    fn_id: u32,
) -> (bool, BTreeMap<String, bool>) {
    let encoded = EncodedGraph::build_scoped(graph, session.ctx(), fn_id);
    let entry_block = entry_block_for_function(graph, fn_id);
    let entry_block = match entry_block.and_then(|id| encoded.bb.get(&id)) {
        Some(bb) => bb.clone(),
        None => return (false, BTreeMap::new()),
    };

    let error_nodes: Vec<u32> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::ErrorToFunction && e.dst == fn_id)
        .map(|e| e.src)
        .collect();
    if error_nodes.is_empty() {
        return (false, BTreeMap::new());
    }

    let reachable_blocks = reachable_blocks_for_function(graph, fn_id);

    let graph_hash = function_graph_hash(graph, fn_id);
    let solver = session.solver();
    solver.push();
    encoded.assert_all(&solver);
    solver.assert(&entry_block);
    for err_id in error_nodes {
        if !error_structurally_reachable(graph, err_id, &reachable_blocks) {
            continue;
        }
        let key = reachability_key(fn_id, err_id, &graph_hash);
        if let Ok(cache) = session.cache().lock() {
            if let Some(entry) = cache.get(&key, &graph_hash) {
                if entry.result == "sat" {
                    let path = entry.model.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                    solver.pop(1);
                    return (true, path);
                }
                if entry.result == "unsat" || entry.result == "unknown" {
                    continue;
                }
            }
        }
        let err = match encoded.err.get(&err_id) {
            Some(e) => e.clone(),
            None => continue,
        };
        solver.push();
        solver.assert(&err);
        let result = solver.check();
        match result {
            SatResult::Sat => {
                let model = solver.get_model();
                let mut path = BTreeMap::new();
                if let Some(model) = model {
                    for (id, bb) in &encoded.bb {
                        if let Some(val) = model.eval(bb, true).and_then(|v| v.as_bool()) {
                            path.insert(format!("bb_{}", id), val);
                        }
                    }
                }
                if let Ok(mut cache) = session.cache().lock() {
                    let entry = CacheEntry {
                        result: "sat".to_string(),
                        model: serde_json::to_value(&path).ok(),
                        graph_hash: graph_hash.clone(),
                        timestamp: now_ts(),
                    };
                    cache.insert(key, entry);
                }
                solver.pop(1);
                solver.pop(1);
                return (true, path);
            }
            SatResult::Unsat | SatResult::Unknown => {
                if let Ok(mut cache) = session.cache().lock() {
                    let entry = CacheEntry {
                        result: if matches!(result, SatResult::Unsat) { "unsat".to_string() } else { "unknown".to_string() },
                        model: None,
                        graph_hash: graph_hash.clone(),
                        timestamp: now_ts(),
                    };
                    cache.insert(key, entry);
                }
                solver.pop(1);
            }
        }
    }
    solver.pop(1);
    (false, BTreeMap::new())
}

fn entry_block_for_function(graph: &AnalysisGraph, fn_id: u32) -> Option<u32> {
    let mut blocks: Vec<&crate::loader::Node> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::HasBlock && e.src == fn_id)
        .filter_map(|e| graph.id_to_index.get(&e.dst).and_then(|&i| graph.nodes.get(i)))
        .filter(|n| n.kind == NodeKind::BasicBlock)
        .collect();
    if blocks.is_empty() {
        return None;
    }
    blocks.sort_by_key(|n| n.symbol.clone());
    blocks.first().map(|n| n.id)
}

fn reachable_blocks_for_function(graph: &AnalysisGraph, fn_id: u32) -> BTreeMap<u32, bool> {
    let mut blocks: Vec<u32> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::HasBlock && e.src == fn_id)
        .filter_map(|e| graph.id_to_index.get(&e.dst).and_then(|&i| graph.nodes.get(i)))
        .filter(|n| n.kind == NodeKind::BasicBlock)
        .map(|n| n.id)
        .collect();
    blocks.sort();
    if blocks.is_empty() {
        return BTreeMap::new();
    }

    let mut index: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, id) in blocks.iter().enumerate() {
        index.insert(*id, i);
    }

    let mut adj = vec![Vec::new(); blocks.len()];
    for e in &graph.edges {
        if e.kind == EdgeKind::Flow {
            if let (Some(&u), Some(&v)) = (index.get(&e.src), index.get(&e.dst)) {
                adj[u].push(v);
            }
        }
    }
    let csr = Csr::from_adj(&adj);

    let entry_block = entry_block_for_function(graph, fn_id);
    let entry_idx = entry_block.and_then(|id| index.get(&id).copied());
    let mut reachable = BTreeMap::new();
    if let Some(root) = entry_idx {
        let mask = reachability_gpu(&csr, &[root]);
        for (i, id) in blocks.iter().enumerate() {
            reachable.insert(*id, mask.get(i).copied().unwrap_or(false));
        }
    } else {
        for id in blocks {
            reachable.insert(id, false);
        }
    }
    reachable
}

fn error_structurally_reachable(graph: &AnalysisGraph, err_id: u32, reachable_blocks: &BTreeMap<u32, bool>) -> bool {
    let mut has_block_edge = false;
    for e in &graph.edges {
        if e.kind == EdgeKind::ErrorToBlock && e.src == err_id {
            has_block_edge = true;
            if reachable_blocks.get(&e.dst).copied().unwrap_or(false) {
                return true;
            }
        }
    }
    if !has_block_edge {
        return true;
    }
    false
}
