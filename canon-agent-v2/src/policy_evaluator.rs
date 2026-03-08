use super::graph_algo::graph_analysis_normalize_features;
use super::policy::{
    PolicyModelPolicyBias, ExecutionPolicyDecision, ExecutionPolicyModel,
};
pub struct PolicyEvalPolicyOutcome {
    pub normalized: Vec<f64>,
    pub bias: PolicyModelPolicyBias,
    pub decision: ExecutionPolicyDecision,
    pub weight_norm: f64,
}
pub fn evaluate_policy(
    features: &super::graph_algo::GraphFeatureVector,
    max_nodes: usize,
    max_edges: usize,
) -> PolicyEvalPolicyOutcome {
    let normalized = graph_analysis_normalize_features(features, max_nodes, max_edges);
    let model = ExecutionPolicyModel::load_default();
    let bias = model.predict(&normalized);
    let decision = model.decide(&normalized);
    PolicyEvalPolicyOutcome {
        normalized,
        bias,
        decision,
        weight_norm: model.weight_norm(),
    }
}
