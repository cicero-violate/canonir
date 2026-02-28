use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use super::ids::{JudgmentId, JudgmentPredicateId, ProposalId};
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: JudgmentId,
    pub proposal: ProposalId,
    pub predicate: JudgmentPredicateId,
    pub decision: JudgmentDecision,
    pub rationale: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: JudgmentPredicateId,
    pub description: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentDecision {
    Accept,
    Reject,
}
