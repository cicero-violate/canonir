use crate::invariants::invariant_generator::InvariantCandidate;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SatResult {
    pub rule: String,
    pub valid: bool,
}

pub fn validate_candidates(candidates: &[InvariantCandidate]) -> Vec<SatResult> {
    candidates
        .iter()
        .map(|c| SatResult {
            rule: c.rule.clone(),
            valid: c.support >= 0.98 && c.confidence >= 0.99,
        })
        .collect()
}
