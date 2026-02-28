use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{LearningId, ProposalId};

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Learning {
    pub id: LearningId,
    pub proposal: ProposalId,
    pub new_rules: Vec<String>,
    pub notes: String,
    pub proof_object_hash: Option<String>,
}
