use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{PolicyParameterId, ProofId, TickEpochId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyParameters {
    pub id: PolicyParameterId,
    pub version: u32,
    pub epoch: TickEpochId,
    pub learning_rate: f64,
    pub discount_factor: f64,
    pub entropy_weight: f64,
    pub reward_baseline: f64,
    #[serde(default)]
    pub proof_id: Option<ProofId>,
}

impl PolicyParameters {
    /// Returns the meta-tick interval from this policy (default: 10).
    /// meta_tick fires every this many TickEpoch completions.
    pub fn meta_tick_interval(&self) -> u64 {
        // Derived from entropy_weight: higher weight → more frequent meta-ticks.
        // interval = clamp(round(10 / (entropy_weight + 0.1)), 2, 50)
        let raw = (10.0 / (self.entropy_weight + 0.1)).round() as u64;
        raw.clamp(2, 50)
    }
}
