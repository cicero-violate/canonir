use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DeltaId, TickId, Word};

/// Predictive world model attached to CanonicalIr.
/// Layer 2 — minimal deterministic scaffold.
/// Snapshot of a particular tick's state for reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateSnapshot {
    pub tick: TickId,
    pub delta_ids: Vec<DeltaId>,
    #[serde(default)]
    pub description: String,
}

/// A prediction head records the planner's rollout for a given tick.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionHead {
    pub tick: TickId,
    pub horizon: u32,
    pub estimated_reward: f64,
    pub predicted_deltas: Vec<DeltaId>,
    pub predicted_state: StateSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct WorldModel {
    /// Monotonic version for schema evolution.
    pub version: String,

    /// Last observed tick.
    pub last_tick: Option<TickId>,

    /// Hash or identifier of the state snapshot used for prediction.
    pub state_root: Option<Word>,

    /// Latest reconciliation snapshot derived from actual execution.
    #[serde(default)]
    pub state_snapshot: Option<StateSnapshot>,

    /// Rolling prediction head entries.
    #[serde(default)]
    pub prediction_head: Vec<PredictionHead>,

    /// Rolling prediction records.
    pub predictions: Vec<PredictionRecord>,
}

impl WorldModel {
    pub fn new() -> Self {
        Self { version: "0.1.0-world-model".to_string(), last_tick: None, state_root: None, state_snapshot: None, prediction_head: Vec::new(), predictions: Vec::new() }
    }

    pub fn push_prediction_head(&mut self, head: PredictionHead) {
        self.prediction_head.push(head);
    }

    pub fn record_prediction(&mut self, record: PredictionRecord, snapshot: StateSnapshot) {
        self.last_tick = Some(record.tick.clone());
        self.state_snapshot = Some(snapshot);
        self.predictions.push(record);
    }
}

/// Stores predicted vs actual outcome for a tick.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionRecord {
    /// Tick being predicted.
    pub tick: TickId,

    /// Predicted deltas (IDs only — no duplication of payload).
    pub predicted_deltas: Vec<DeltaId>,

    /// Actual realized deltas.
    pub actual_deltas: Vec<DeltaId>,

    /// Absolute prediction error (|predicted - actual|).
    pub error: f64,
}

impl PredictionRecord {
    pub fn new(tick: TickId, predicted_deltas: Vec<DeltaId>, actual_deltas: Vec<DeltaId>) -> Self {
        let error = (predicted_deltas.len() as i64 - actual_deltas.len() as i64).unsigned_abs() as f64;

        Self { tick, predicted_deltas, actual_deltas, error }
    }
}
