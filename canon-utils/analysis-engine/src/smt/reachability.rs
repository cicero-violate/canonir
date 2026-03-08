use crate::loader::{AnalysisGraph, EdgeKind, NodeKind};
use crate::smt::encoder::EncodedGraph;
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
    encoded: &EncodedGraph,
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
        let (reachable, path) = check_function_errors(session, graph, encoded, node_id);
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
    encoded: &EncodedGraph,
    fn_id: u32,
) -> (bool, BTreeMap<String, bool>) {
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

    for err_id in error_nodes {
        let err = match encoded.err.get(&err_id) {
            Some(e) => e.clone(),
            None => continue,
        };
        let solver = session.solver();
        solver.push();
        encoded.assert_all(&solver);
        solver.assert(&entry_block);
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
                solver.pop(1);
                return (true, path);
            }
            SatResult::Unsat | SatResult::Unknown => {
                solver.pop(1);
            }
        }
    }
    (false, BTreeMap::new())
}

fn entry_block_for_function(graph: &AnalysisGraph, fn_id: u32) -> Option<u32> {
    let mut blocks: Vec<&crate::loader::Node> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::HasBlock && e.src == fn_id)
        .filter_map(|e| graph.nodes.get(e.dst as usize))
        .filter(|n| n.kind == NodeKind::BasicBlock)
        .collect();
    if blocks.is_empty() {
        return None;
    }
    blocks.sort_by_key(|n| n.symbol.clone());
    blocks.first().map(|n| n.id)
}
