use super::graph_algo::normalize_features;
use super::policy::{PolicyBias, PolicyDecision, PolicyModel};

pub struct PolicyOutcome {
    pub normalized: Vec<f64>,
    pub bias: PolicyBias,
    pub decision: PolicyDecision,
    pub weight_norm: f64,
}

pub fn evaluate(features: &super::graph_algo::FeatureVector, max_nodes: usize, max_edges: usize) -> PolicyOutcome {
    let normalized = normalize_features(features, max_nodes, max_edges);
    let model = PolicyModel::load_default();
    let bias = model.predict(&normalized);
    let decision = model.decide(&normalized);
    PolicyOutcome { normalized, bias, decision, weight_norm: model.weight_norm() }
}
