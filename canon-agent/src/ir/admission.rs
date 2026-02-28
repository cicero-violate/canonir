use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use super::ids::{AdmissionId, AppliedDeltaId, DeltaId, TickId};
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct AdmissionPolicy {
    pub id: AdmissionId,
    pub judgment: super::ids::JudgmentId,
    pub tick: TickId,
    pub delta_ids: Vec<DeltaId>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct AppliedDeltaRecord {
    pub id: AppliedDeltaId,
    pub admission: AdmissionId,
    pub delta: DeltaId,
    pub order: u64,
}
