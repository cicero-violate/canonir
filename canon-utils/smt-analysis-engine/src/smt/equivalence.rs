use crate::loader::{AnalysisGraph, NodeKind};
use crate::smt::cache::{equivalence_key, equivalence_graph_hash, CacheEntry, now_ts};
use crate::smt::encoder::EncodedGraph;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use z3::SatResult;
use z3::ast::Ast;

#[derive(Debug, Serialize)]
pub struct EquivalenceResult {
    pub kind: String,
    pub a: u32,
    pub b: u32,
    pub smt_equivalent: bool,
    pub distinguishing_input: Option<Value>,
}

pub fn check_equivalence(
    session: &crate::smt::SmtSession,
    graph: &AnalysisGraph,
    candidates: &Value,
) -> Vec<EquivalenceResult> {
    let mut results = Vec::new();
    let pairs = candidates.get("pairs").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut fn_prefix: BTreeMap<u32, String> = BTreeMap::new();
    for node in &graph.nodes {
        if matches!(node.kind, NodeKind::Function | NodeKind::Method) {
            let prefix = node.symbol.trim_end_matches("::fn").to_string();
            fn_prefix.insert(node.id, prefix);
        }
    }

    for pair in pairs {
        let a = pair.get("a").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let b = pair.get("b").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if a == 0 || b == 0 {
            continue;
        }

        let graph_hash = equivalence_graph_hash(graph, a, b);
        let key = equivalence_key(a, b, &graph_hash);
        if let Ok(cache) = session.cache().lock() {
            if let Some(entry) = cache.get(&key, &graph_hash) {
                let eq = entry.result == "unsat";
                results.push(EquivalenceResult {
                    kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                    a,
                    b,
                    smt_equivalent: eq,
                    distinguishing_input: entry.model,
                });
                continue;
            }
        }

        let vars_a = vars_for_function(graph, &fn_prefix, a);
        let vars_b = vars_for_function(graph, &fn_prefix, b);
        let params_a = params_for_function(graph, &fn_prefix, a);
        let params_b = params_for_function(graph, &fn_prefix, b);
        let returns_a = returns_for_function(graph, &fn_prefix, a);
        let returns_b = returns_for_function(graph, &fn_prefix, b);
        if vars_a.is_empty()
            || vars_b.is_empty()
            || vars_a.len() != vars_b.len()
            || params_a.len() != params_b.len()
            || returns_a.len() != returns_b.len()
        {
            results.push(EquivalenceResult {
                kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                a,
                b,
                smt_equivalent: false,
                distinguishing_input: None,
            });
            continue;
        }

        let encoded_a = EncodedGraph::build_scoped(graph, session.ctx(), a);
        let encoded_b = EncodedGraph::build_scoped(graph, session.ctx(), b);
        let solver = session.solver();
        encoded_a.assert_all(&solver);
        encoded_b.assert_all(&solver);
        solver.push();
        for (lhs, rhs) in params_a.iter().zip(params_b.iter()) {
            if let (Some(l), Some(r)) = (encoded_a.var.get(lhs), encoded_b.var.get(rhs)) {
                solver.assert(&l._eq(r));
            }
        }
        let mut output_diffs = Vec::new();
        for (lhs, rhs) in returns_a.iter().zip(returns_b.iter()) {
            if let (Some(l), Some(r)) = (encoded_a.var.get(lhs), encoded_b.var.get(rhs)) {
                output_diffs.push(l._eq(r).not());
            }
        }
        if output_diffs.is_empty() {
            solver.pop(1);
            results.push(EquivalenceResult {
                kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                a,
                b,
                smt_equivalent: false,
                distinguishing_input: None,
            });
            continue;
        }
        let diff_refs: Vec<&z3::ast::Bool<'_>> = output_diffs.iter().collect();
        solver.assert(&z3::ast::Bool::or(session.ctx(), &diff_refs));
        let verdict = match solver.check() {
            SatResult::Unsat => true,
            SatResult::Sat => false,
            SatResult::Unknown => false,
        };
        let distinguishing_input = if !verdict {
            let model = solver.get_model();
            model.map(|m| {
                let mut vals = BTreeMap::new();
                for (ra, rb) in returns_a.iter().zip(returns_b.iter()) {
                    if let (Some(l), Some(r)) = (encoded_a.var.get(ra), encoded_b.var.get(rb)) {
                        let lv = m.eval(l, true).and_then(|v| v.as_u64());
                        let rv = m.eval(r, true).and_then(|v| v.as_u64());
                        vals.insert(format!("val_{}", ra), lv.map(|v| v.to_string()).unwrap_or_default());
                        vals.insert(format!("val_{}", rb), rv.map(|v| v.to_string()).unwrap_or_default());
                    }
                }
                serde_json::to_value(vals).unwrap_or(Value::Null)
            })
        } else {
            None
        };
        if let Ok(mut cache) = session.cache().lock() {
            let entry = CacheEntry {
                result: if verdict { "unsat".to_string() } else { "sat".to_string() },
                model: distinguishing_input.clone(),
                graph_hash: graph_hash.clone(),
                timestamp: now_ts(),
            };
            cache.insert(key, entry);
        }
        results.push(EquivalenceResult {
            kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            a,
            b,
            smt_equivalent: verdict,
            distinguishing_input,
        });
        solver.pop(1);
    }
    results
}

fn vars_for_function(graph: &AnalysisGraph, prefixes: &BTreeMap<u32, String>, fn_id: u32) -> Vec<u32> {
    let Some(prefix) = prefixes.get(&fn_id) else {
        return Vec::new();
    };
    let mut vars: Vec<(String, u32)> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Variable | NodeKind::Param))
        .filter(|n| n.symbol.starts_with(prefix))
        .map(|n| (n.symbol.clone(), n.id))
        .collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    vars.into_iter().map(|(_, id)| id).collect()
}

fn params_for_function(graph: &AnalysisGraph, prefixes: &BTreeMap<u32, String>, fn_id: u32) -> Vec<u32> {
    let Some(prefix) = prefixes.get(&fn_id) else {
        return Vec::new();
    };
    let mut vars: Vec<(String, u32)> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Param)
        .filter(|n| n.symbol.starts_with(prefix))
        .map(|n| (n.symbol.clone(), n.id))
        .collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    vars.into_iter().map(|(_, id)| id).collect()
}

fn returns_for_function(graph: &AnalysisGraph, prefixes: &BTreeMap<u32, String>, fn_id: u32) -> Vec<u32> {
    let Some(prefix) = prefixes.get(&fn_id) else {
        return Vec::new();
    };
    let mut vars: Vec<(String, u32)> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Variable)
        .filter(|n| n.symbol.starts_with(prefix))
        .filter(|n| n.symbol.contains("::_0") || n.symbol.ends_with("::ret"))
        .map(|n| (n.symbol.clone(), n.id))
        .collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    vars.into_iter().map(|(_, id)| id).collect()
}
