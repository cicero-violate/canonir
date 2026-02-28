use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::ErrorId;

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ErrorArtifact {
    pub id: ErrorId,
    pub rule: String,
    pub message: String,
}
