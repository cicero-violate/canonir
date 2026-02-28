use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use super::{
    ids::{
        DeltaId, ExecutionRecordId, FunctionId, JudgmentId, JudgmentPredicateId,
        LoopPolicyId, PlanId, TickEpochId, TickGraphId, TickId,
    },
    word::Word,
};
use crate::ir::PolicyParameters;
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct LoopPolicy {
    pub id: LoopPolicyId,
    pub graph: TickGraphId,
    pub continuation: JudgmentPredicateId,
    pub max_ticks: Option<u64>,
    pub description: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Tick {
    pub id: TickId,
    pub graph: TickGraphId,
    pub input_state: Vec<DeltaId>,
    pub output_deltas: Vec<DeltaId>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEpoch {
    pub id: TickEpochId,
    pub ticks: Vec<TickId>,
    pub parent_epoch: Option<TickEpochId>,
    #[serde(default)]
    pub entropy_reduction: f64,
    #[serde(default)]
    pub policy_snapshot: Option<PolicySnapshot>,
}
/// Snapshot captured at epoch boundaries.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct PolicySnapshot {
    pub epoch: TickEpochId,
    pub parameters: PolicyParameters,
    pub reward_at_snapshot: f64,
    pub delta_ids: Vec<DeltaId>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub id: PlanId,
    pub judgment: JudgmentId,
    pub steps: Vec<FunctionId>,
    pub expected_deltas: Vec<DeltaId>,
    #[serde(default)]
    pub search_depth: u32,
    #[serde(default)]
    pub utility_estimate: f64,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    pub id: ExecutionRecordId,
    pub tick: TickId,
    pub plan: PlanId,
    pub outcome_deltas: Vec<DeltaId>,
    #[serde(default)]
    pub errors: Vec<ExecutionError>,
    #[serde(default)]
    pub events: Vec<ExecutionEvent>,
    /// Scalar utility U(s_t) computed after this execution.
    /// None until the reward computation pass has run.
    #[serde(default)]
    pub reward: Option<f64>,
    /// Utility predicted by planner before execution.
    #[serde(default)]
    pub planned_utility: Option<f64>,
    /// Search depth used by planner.
    #[serde(default)]
    pub planning_depth: Option<u32>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct ExecutionError {
    pub code: Word,
    pub message: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionEvent {
    Stdout { text: String },
    Stderr { text: String },
    Artifact { path: String, hash: String },
    Error { code: Word, message: String },
}
