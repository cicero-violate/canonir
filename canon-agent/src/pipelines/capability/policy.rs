use serde::{Deserialize, Serialize};
use std::path::Path;
use rand::Rng;


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
    pub(crate) weights: PolicyWeights,
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

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(&self.weights).unwrap_or_default();
        std::fs::write(path, json)
    }

    pub fn predict(&self, features: &[f64]) -> PolicyBias {
        let fv = features;
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

    pub fn weight_norm(&self) -> f64 {
        let all = self.weights.planner_bias.iter()
            .chain(self.weights.node_add_bias.iter())
            .chain(self.weights.edge_add_bias.iter())
            .chain(self.weights.rewrite_bias.iter());
        all.map(|v| v * v).sum::<f64>().sqrt()
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

pub fn smooth_bias(prev: Option<&PolicyBias>, next: PolicyBias) -> PolicyBias {
    if let Some(p) = prev {
        PolicyBias {
            planner_bias: 0.8 * p.planner_bias + 0.2 * next.planner_bias,
            node_add_bias: 0.8 * p.node_add_bias + 0.2 * next.node_add_bias,
            edge_add_bias: 0.8 * p.edge_add_bias + 0.2 * next.edge_add_bias,
            rewrite_bias: 0.8 * p.rewrite_bias + 0.2 * next.rewrite_bias,
        }
    } else {
        next
    }
}

pub fn maybe_explore(mut bias: PolicyBias, epsilon: f64) -> PolicyBias {
    let mut rng = rand::thread_rng();
    if rng.r#gen::<f64>() < epsilon {
        bias.planner_bias = 0.0;
        bias.node_add_bias = 0.0;
        bias.edge_add_bias = 0.0;
        bias.rewrite_bias = 0.0;
    }
    bias
}
