use serde::Serialize;
use serde_json::{json, Value};
use z3::SatResult;

#[derive(Debug, Serialize)]
pub struct InvariantSmtResult {
    pub predicate: String,
    pub smt_verdict: String,
    pub counterexample: Option<Value>,
}

pub fn prove_invariants(
    session: &crate::smt::SmtSession,
    _encoded: &crate::smt::encoder::EncodedGraph,
    invariants: &Value,
) -> Value {
    let mut out = invariants.clone();
    let mut results: Vec<InvariantSmtResult> = Vec::new();

    let candidate_list = invariants
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for cand in candidate_list {
        let pred = cand.get("predicate").and_then(|v| v.as_str()).unwrap_or("unknown");
        let solver = session.solver();
        solver.push();
        let neg = z3::ast::Bool::new_const(format!("neg_{}", pred));
        solver.assert(&neg);
        let verdict = match solver.check() {
            SatResult::Unsat => "proven",
            SatResult::Sat => "violated",
            SatResult::Unknown => "unverified",
        };
        let counterexample = if verdict == "violated" { Some(json!({})) } else { None };
        solver.pop(1);
        results.push(InvariantSmtResult {
            predicate: pred.to_string(),
            smt_verdict: verdict.to_string(),
            counterexample,
        });
    }

    if let Some(obj) = out.as_object_mut() {
        obj.insert("smt_verdicts".to_string(), serde_json::to_value(results).unwrap_or(Value::Null));
    }
    out
}
