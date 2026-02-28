use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{ExecutionRecordId, RewardRecordId, TickId};

/// Scalar utility value recorded after a tick execution.
/// r_t = U(s_t)
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct RewardRecord {
    pub id: RewardRecordId,
    /// The tick this reward was computed for.
    pub tick: TickId,
    /// The execution record that produced this reward.
    pub execution: ExecutionRecordId,
    /// Scalar utility value U(s_t).
    pub reward: f64,
    /// Signed delta from prior reward: r_t - r_{t-1}.
    /// None for the first record in an epoch.
    #[serde(default)]
    pub delta: Option<f64>,
}

/// Which utility function family produced this reward.
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UtilityKind {
    /// Constant scalar placeholder until a real formula is registered.
    Scalar,
}
