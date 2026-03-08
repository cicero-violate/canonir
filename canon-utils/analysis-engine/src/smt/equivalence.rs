use serde::Serialize;
use serde_json::Value;
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
    _encoded: &crate::smt::encoder::EncodedGraph,
    candidates: &Value,
) -> Vec<EquivalenceResult> {
    let mut results = Vec::new();
    let pairs = candidates.get("pairs").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for pair in pairs {
        let a = pair.get("a").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let b = pair.get("b").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if a == 0 || b == 0 {
            continue;
        }
        let solver = session.solver();
        solver.push();
        let neq = z3::ast::Bool::new_const(format!("neq_{}_{}", a, b));
        solver.assert(&neq);
        let verdict = match solver.check() {
            SatResult::Unsat => true,
            SatResult::Sat => false,
            SatResult::Unknown => false,
        };
        solver.pop(1);
        results.push(EquivalenceResult {
            kind: pair.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            a,
            b,
            smt_equivalent: verdict,
            distinguishing_input: None,
        });
    }
    results
}
