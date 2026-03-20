use crate::semantics::pattern_mining::PatternRule;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct InvariantCandidate {
    pub rule: String,
    pub support: f64,
    pub confidence: f64,
}

pub fn generate_candidates(patterns: &[PatternRule]) -> Vec<InvariantCandidate> {
    patterns.iter().filter(|p| p.support >= 0.98 && p.confidence >= 0.99).map(|p| InvariantCandidate { rule: p.rule.clone(), support: p.support, confidence: p.confidence }).collect()
}
