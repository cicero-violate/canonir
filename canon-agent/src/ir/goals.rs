use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ir::ids::{GoalMutationId, ProofId, ProposalId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalMutation {
    pub id: GoalMutationId,
    pub original_goal: String,
    pub proposed_goal: String,
    pub invariant_proof_ids: Vec<ProofId>,
    pub proposal_id: ProposalId,
    #[serde(default)]
    pub judgment_id: Option<String>,
    pub status: GoalMutationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalMutationStatus {
    Proposed,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalDriftMetric {
    pub mutation_id: GoalMutationId,
    pub cosine_distance: f64,
    pub keyword_overlap: f64,
    pub within_bound: bool,
    pub bound_theta: f64,
}

pub fn compute_goal_drift(original: &str, proposed: &str, theta: f64) -> GoalDriftMetric {
    fn tokenize(text: &str) -> std::collections::HashSet<String> {
        text.split_whitespace().map(|token| token.trim_matches(|c: char| !c.is_alphanumeric())).filter(|token| !token.is_empty()).map(|token| token.to_lowercase()).collect()
    }

    let original_tokens = tokenize(original);
    let proposed_tokens = tokenize(proposed);
    let union_size = original_tokens.union(&proposed_tokens).count().max(1);
    let intersection_size = original_tokens.intersection(&proposed_tokens).count();
    let keyword_overlap = intersection_size as f64 / union_size as f64;
    let cosine_distance = 1.0 - keyword_overlap;
    let within_bound = keyword_overlap >= 1.0 - theta;

    GoalDriftMetric { mutation_id: GoalMutationId::default(), cosine_distance, keyword_overlap, within_bound, bound_theta: theta }
}
