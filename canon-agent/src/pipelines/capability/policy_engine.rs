use super::config::CapabilityConfig;
use super::graph_algo::normalize_features;
use super::policy::{PolicyBias, PolicyDecision, PolicyModel};

pub struct PolicyOutcome {
    pub normalized: Vec<f64>,
    pub bias: PolicyBias,
    pub decision: PolicyDecision,
    pub weight_norm: f64,
}

pub fn evaluate(features: &super::graph_algo::FeatureVector, config: &CapabilityConfig) -> PolicyOutcome {
    let normalized = normalize_features(features, config.max_nodes, config.max_nodes.saturating_mul(4));
    let model = PolicyModel::load_default();
    let bias = model.predict(&normalized);
    let decision = model.decide(&normalized);
    PolicyOutcome { normalized, bias, decision, weight_norm: model.weight_norm() }
}
