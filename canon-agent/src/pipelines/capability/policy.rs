use serde::{Deserialize, Serialize};
use std::path::Path;

use super::graph_algo::FeatureVector;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyWeights {
    #[serde(default)]
    pub planner_bias: Vec<f64>,
    #[serde(default)]
    pub node_add_bias: Vec<f64>,
    #[serde(default)]
    pub edge_add_bias: Vec<f64>,
    #[serde(default)]
    pub rewrite_bias: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct PolicyModel {
    weights: PolicyWeights,
}

#[derive(Debug, Clone)]
pub struct PolicyBias {
    pub planner_bias: f64,
    pub node_add_bias: f64,
    pub edge_add_bias: f64,
    pub rewrite_bias: f64,
}

impl PolicyModel {
    pub fn load_default() -> Self {
        let path = Path::new("/workspace/ai_sandbox/canon/agent_logs/policy_weights.json");
        Self::load(path)
    }

    pub fn load(path: &Path) -> Self {
        let weights = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<PolicyWeights>(&s).ok())
            .unwrap_or_else(|| PolicyWeights {
                planner_bias: Vec::new(),
                node_add_bias: Vec::new(),
                edge_add_bias: Vec::new(),
                rewrite_bias: Vec::new(),
            });
        Self { weights }
    }

    pub fn predict(&self, features: &FeatureVector) -> PolicyBias {
        let fv = features.to_vec();
        let dot = |w: &Vec<f64>| -> f64 {
            w.iter().zip(fv.iter()).map(|(a, b)| a * b).sum()
        };
        PolicyBias {
            planner_bias: dot(&self.weights.planner_bias),
            node_add_bias: dot(&self.weights.node_add_bias),
            edge_add_bias: dot(&self.weights.edge_add_bias),
            rewrite_bias: dot(&self.weights.rewrite_bias),
        }
    }
}

pub fn format_bias(bias: &PolicyBias) -> String {
    if bias.planner_bias == 0.0 && bias.node_add_bias == 0.0 && bias.edge_add_bias == 0.0 && bias.rewrite_bias == 0.0 {
        return String::new();
    }
    format!(
        "Policy bias:\nplanner_bias={:.3}\nnode_add_bias={:.3}\nedge_add_bias={:.3}\nrewrite_bias={:.3}\n\
Prefer actions with positive bias and avoid strongly negative bias.\n",
        bias.planner_bias,
        bias.node_add_bias,
        bias.edge_add_bias,
        bias.rewrite_bias
    )
}
