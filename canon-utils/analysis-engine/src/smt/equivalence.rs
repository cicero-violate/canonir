use crate::loader::{AnalysisGraph, NodeKind};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use z3::SatResult;

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
    encoded: &crate::smt::encoder::EncodedGraph,
    candidates: &Value,
) -> Vec<EquivalenceResult> {
    let mut results = Vec::new();
    let pairs = candidates.get("pairs").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let solver = session.solver();
    solver.push();
    encoded.assert_all(&solver);

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

        let vars_a = vars_for_function(graph, &fn_prefix, a);
        let vars_b = vars_for_function(graph, &fn_prefix, b);
        if vars_a.is_empty() || vars_b.is_empty() || vars_a.len() != vars_b.len() {
            results.push(EquivalenceResult {
                kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                a,
                b,
                smt_equivalent: false,
                distinguishing_input: None,
            });
            continue;
        }

        solver.push();
        for (lhs, rhs) in vars_a.iter().zip(vars_b.iter()) {
            if let (Some(l), Some(r)) = (encoded.var.get(lhs), encoded.var.get(rhs)) {
                solver.assert(&l.eq(r));
            }
        }
        let mut diffs = Vec::new();
        for (lhs, rhs) in vars_a.iter().zip(vars_b.iter()) {
            if let (Some(l), Some(r)) = (encoded.var.get(lhs), encoded.var.get(rhs)) {
                diffs.push(l.eq(r).not());
            }
        }
        if !diffs.is_empty() {
            solver.assert(&z3::ast::Bool::or(&diffs));
        }
        let verdict = match solver.check() {
            SatResult::Unsat => true,
            SatResult::Sat => false,
            SatResult::Unknown => false,
        };
        let distinguishing_input = if !verdict {
            let model = solver.get_model();
            model.map(|m| {
                let mut vals = BTreeMap::new();
                for (lhs, rhs) in vars_a.iter().zip(vars_b.iter()) {
                    if let (Some(l), Some(r)) = (encoded.var.get(lhs), encoded.var.get(rhs)) {
                        let lv = m.eval(l, true).and_then(|v| v.as_u64());
                        let rv = m.eval(r, true).and_then(|v| v.as_u64());
                        vals.insert(format!("val_{}", lhs), lv.map(|v| v.to_string()).unwrap_or_default());
                        vals.insert(format!("val_{}", rhs), rv.map(|v| v.to_string()).unwrap_or_default());
                    }
                }
                serde_json::to_value(vals).unwrap_or(Value::Null)
            })
        } else {
            None
        };
        solver.pop(1);
        results.push(EquivalenceResult {
            kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            a,
            b,
            smt_equivalent: verdict,
            distinguishing_input,
        });
    }
    solver.pop(1);
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
