use super::capability::CapabilityMode;
use super::capability_cost::CapabilityCostCapabilityCostTable;
use super::config::CapabilityConfig;
use super::dag::{ExecutionGraph, ExecutionNode};
use super::graph_algo::GraphFeatureVector;
pub fn scheduler_scoring_score_ready_nodes(
    ready_ids: &[String],
    graph: &ExecutionGraph,
    features: &GraphFeatureVector,
    cost_table: &CapabilityCostCapabilityCostTable,
    execution_preference: f64,
    config: &CapabilityConfig,
) -> Vec<(String, f64)> {
    let mut by_id = std::collections::HashMap::new();
    for n in &graph.nodes {
        by_id.insert(n.id.as_str(), n);
    }
    ready_ids
        .iter()
        .filter_map(|id| {
            by_id
                .get(id.as_str())
                .map(|n| (
                    id.clone(),
                    scheduler_scoring_score_node(
                        n,
                        features,
                        cost_table,
                        execution_preference,
                        config,
                    ),
                ))
        })
        .collect()
}
fn scheduler_scoring_score_node(
    node: &ExecutionNode,
    features: &GraphFeatureVector,
    cost_table: &CapabilityCostCapabilityCostTable,
    execution_preference: f64,
    config: &CapabilityConfig,
) -> f64 {
    let base = node.priority as f64;
    let completion = features.completion_velocity;
    let unblock = node
        .required_capabilities
        .iter()
        .any(|c| c.class() == CapabilityMode::Observe) as u8 as f64;
    let retry = node.readonly_fail_count as f64;
    let cost = cost_table
        .node_cost(
            &node.required_capabilities,
            config.cost_latency_weight,
            config.cost_failure_weight,
        );
    let w1 = 1.0;
    let w2 = 0.4;
    let w3 = 0.6;
    let w4 = 0.8;
    let w5 = 1.0;
    (w1 * base * (1.0 + execution_preference)) + (w2 * completion) + (w3 * unblock)
        - (w4 * retry) - (w5 * cost)
}
