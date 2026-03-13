use crate::smt::loader::AnalysisGraph;
use crate::smt::cache::{invariant_graph_hash, invariant_key, CacheEntry, now_ts};
use crate::smt::encoder::EncodedGraph;
use serde::Serialize;
use serde_json::{json, Value};
use z3::SatResult;
use z3::ast::Ast;

#[derive(Debug, Serialize)]
pub struct InvariantSmtResult {
    pub predicate: String,
    pub smt_verdict: String,
    pub counterexample: Option<Value>,
}

pub fn prove_invariants(
    session: &crate::smt::SmtSession,
    graph: &AnalysisGraph,
    invariants: &Value,
) -> Value {
    let encoded = EncodedGraph::build(graph, session.ctx());
    let mut out = invariants.clone();
    let mut results: Vec<InvariantSmtResult> = Vec::new();
    let candidate_graph = graph;

    let candidate_list = invariants
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if candidate_list.is_empty() {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("smt_verdicts".to_string(), serde_json::to_value(results).unwrap_or(Value::Null));
        }
        return out;
    }

    let solver = session.solver();
    solver.push();
    encoded.assert_all(&solver);

    for cand in candidate_list {
        let pred = cand.get("predicate").and_then(|v| v.as_str()).unwrap_or("unknown");
        let graph_hash = invariant_graph_hash(candidate_graph, &cand);
        let key = invariant_key(pred, &graph_hash);
        if let Ok(cache) = session.cache().lock() {
            if let Some(entry) = cache.get(&key, &graph_hash) {
                results.push(InvariantSmtResult {
                    predicate: pred.to_string(),
                    smt_verdict: entry.result,
                    counterexample: entry.model,
                });
                continue;
            }
        }
        solver.push();
        let mut asserted = false;
        if let Some(kind) = cand.get("kind").and_then(|v| v.as_str()) {
            match kind {
                "bb_true" => {
                    if let Some(id) = cand.get("block_id").and_then(|v| v.as_u64()) {
                        if let Some(bb) = encoded.bb.get(&(id as u32)) {
                            solver.assert(&bb.not());
                            asserted = true;
                        }
                    }
                }
                "err_unreachable" => {
                    if let Some(id) = cand.get("error_id").and_then(|v| v.as_u64()) {
                        if let Some(err) = encoded.err.get(&(id as u32)) {
                            solver.assert(err);
                            asserted = true;
                        }
                    }
                }
                "var_eq" => {
                    let lhs = cand.get("lhs").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let rhs = cand.get("rhs").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    if let (Some(l), Some(r)) = (encoded.var.get(&lhs), encoded.var.get(&rhs)) {
                        solver.assert(&l._eq(r).not());
                        asserted = true;
                    }
                }
                _ => {}
            }
        }
        if !asserted {
            let neg = z3::ast::Bool::new_const(encoded.ctx(), format!("neg_{}", pred));
            solver.assert(&neg);
        }
        let verdict = match solver.check() {
            SatResult::Unsat => "proven",
            SatResult::Sat => "violated",
            SatResult::Unknown => "unverified",
        };
        let counterexample = if verdict == "violated" { Some(json!({})) } else { None };
        if let Ok(mut cache) = session.cache().lock() {
            let entry = CacheEntry {
                result: verdict.to_string(),
                model: counterexample.clone(),
                graph_hash,
                timestamp: now_ts(),
            };
            cache.insert(key, entry);
        }
        solver.pop(1);
        results.push(InvariantSmtResult {
            predicate: pred.to_string(),
            smt_verdict: verdict.to_string(),
            counterexample,
        });
    }
    solver.pop(1);

    if let Some(obj) = out.as_object_mut() {
        obj.insert("smt_verdicts".to_string(), serde_json::to_value(results).unwrap_or(Value::Null));
    }
    out
}
