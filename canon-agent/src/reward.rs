//! Reward ledger — closes the learning loop between pipeline outcomes
//! and AgentCallDispatcher trust thresholds.
//!
//! After every PipelineResult the ledger records the outcome, computes
//! a reward signal, updates PolicyParameters via update_policy(), and
//! derives a per-node trust threshold adjustment for the dispatcher.
//!
//! No LLM calls. No unsafe. Pure arithmetic over existing reward infrastructure.
use crate::ir::PolicyParameters;
use crate::runtime::policy_updater::{update_policy, PolicyUpdateError};
use std::collections::HashMap;
/// Outcome of a single pipeline run for a capability node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PipelineNodeOutcome {
    /// Proposal was accepted and mutation applied.
    Accepted { reward: f64 },
    /// Proposal was rejected by the Judge stage.
    Rejected { penalty: f64 },
    /// Pipeline halted before Judge (proof missing, drift exceeded, etc.).
    Halted { penalty: f64 },
}
impl PipelineNodeOutcome {
    pub fn reward_value(&self) -> f64 {
        match self {
            PipelineNodeOutcome::Accepted { reward } => *reward,
            PipelineNodeOutcome::Rejected { penalty } => -*penalty,
            PipelineNodeOutcome::Halted { penalty } => -*penalty,
        }
    }
    pub fn was_accepted(&self) -> bool {
        matches!(self, PipelineNodeOutcome::Accepted { .. })
    }
}
/// Per-node reward history entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeRewardEntry {
    pub node_id: String,
    pub outcome: PipelineNodeOutcome,
    /// Cumulative exponential moving average of reward for this node.
    pub ema_reward: f64,
    /// How many pipeline runs this node has participated in.
    pub run_count: u64,
}
/// Accumulates pipeline outcomes and derives dispatcher trust thresholds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct NodeRewardLedger {
    /// Per-node reward history, keyed by node_id.
    entries: HashMap<String, NodeRewardEntry>,
    /// EMA decay factor α ∈ (0,1). Higher = faster adaptation.
    alpha: f64,
    /// Base trust threshold before reward adjustment.
    base_threshold: f64,
}
impl NodeRewardLedger {
    pub fn new(alpha: f64, base_threshold: f64) -> Self {
        Self { entries: HashMap::new(), alpha, base_threshold }
    }
    /// Record an outcome for a node and update its EMA reward.
    pub fn record(&mut self, node_id: impl Into<String>, outcome: PipelineNodeOutcome) {
        let node_id = node_id.into();
        let reward = outcome.reward_value();
        let entry = self.entries.entry(node_id.clone()).or_insert(NodeRewardEntry { node_id: node_id.clone(), outcome: outcome.clone(), ema_reward: reward, run_count: 0 });
        entry.ema_reward = self.alpha * reward + (1.0 - self.alpha) * entry.ema_reward;
        entry.run_count += 1;
        entry.outcome = outcome;
    }
    /// Derives a trust threshold for a node based on its EMA reward.
    ///
    /// Nodes with positive EMA get lower thresholds (more permissive dispatch).
    /// Nodes with negative EMA get higher thresholds (more scrutiny).
    ///
    /// threshold = clamp(base - ema_reward * scale, 0.1, 0.95)
    pub fn trust_threshold_for(&self, node_id: &str) -> f64 {
        let ema = self.entries.get(node_id).map(|e| e.ema_reward).unwrap_or(0.0);
        let scale = 0.05_f64;
        let raw = self.base_threshold - ema * scale;
        raw.clamp(0.1, 0.95)
    }
    /// Updates PolicyParameters using the aggregate reward across all nodes.
    /// Returns updated parameters or a PolicyUpdateError.
    pub fn update_policy(&self, current: &PolicyParameters) -> Result<PolicyParameters, PolicyUpdateError> {
        let aggregate_reward = self.aggregate_reward();
        update_policy(current, aggregate_reward)
    }
    /// Mean EMA reward across all nodes that have run at least once.
    pub fn aggregate_reward(&self) -> f64 {
        let active: Vec<f64> = self.entries.values().filter(|e| e.run_count > 0).map(|e| e.ema_reward).collect();
        if active.is_empty() {
            return 0.0;
        }
        active.iter().sum::<f64>() / active.len() as f64
    }
    /// Returns all node entries sorted by EMA reward descending.
    /// Useful for surfacing which nodes are performing best.
    pub fn ranked_nodes(&self) -> Vec<&NodeRewardEntry> {
        let mut entries: Vec<&NodeRewardEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| b.ema_reward.partial_cmp(&a.ema_reward).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }
    /// Returns the entry for a node, if it exists.
    pub fn entry_for(&self, node_id: &str) -> Option<&NodeRewardEntry> {
        self.entries.get(node_id)
    }
}
