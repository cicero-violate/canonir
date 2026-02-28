use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::ProofId;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub id: ProofId,
    pub invariant: String,
    pub scope: ProofScope,
    pub evidence: ProofArtifact,
    pub proof_object_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProofArtifact {
    pub uri: String,
    pub hash: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProofScope {
    Structure,
    Execution,
    Law,
}
