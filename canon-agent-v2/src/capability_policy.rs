use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPolicyWeights {
    #[serde(default)]
    pub planner_bias: Vec<f64>,
    #[serde(default)]
    pub node_add_bias: Vec<f64>,
    #[serde(default)]
    pub edge_add_bias: Vec<f64>,
    #[serde(default)]
    pub rewrite_bias: Vec<f64>,
    #[serde(default)]
    pub run_planner_head: Vec<f64>,
    #[serde(default)]
    pub expansion_head: Vec<f64>,
    #[serde(default)]
    pub execution_head: Vec<f64>,
    #[serde(default)]
    pub unblock_head: Vec<f64>,
}
#[derive(Debug, Clone)]
pub struct ExecutionPolicyModel {
    pub(crate) weights: ExecutionPolicyWeights,
}
#[derive(Debug, Clone)]
pub struct PolicyModelPolicyBias {
    pub planner_bias: f64,
    pub node_add_bias: f64,
    pub edge_add_bias: f64,
    pub rewrite_bias: f64,
}
#[derive(Debug, Clone)]
pub struct ExecutionPolicyDecision {
    pub run_planner: bool,
    pub expansion_scale: f64,
    pub prioritize_unblock: bool,
    pub execution_preference: f64,
}
impl ExecutionPolicyModel {
    pub fn load_default() -> Self {
        let path = Path::new(
            "/workspace/ai_sandbox/canon/agent_logs/policy_weights.json",
        );
        Self::snapshot_store_load(path)
    }
    pub fn snapshot_store_load(path: &Path) -> Self {
        let weights = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<ExecutionPolicyWeights>(&s).ok())
            .unwrap_or_else(policy_model_default_weights);
        Self { weights }
    }
    pub fn snapshot_store_save(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(&self.weights).unwrap_or_default();
        std::fs::write(path, json)
    }
    pub fn predict(&self, features: &[f64]) -> PolicyModelPolicyBias {
        let fv = features;
        let dot = |w: &Vec<f64>| -> f64 {
            w.iter().zip(fv.iter()).map(|(a, b)| a * b).sum()
        };
        PolicyModelPolicyBias {
            planner_bias: dot(&self.weights.planner_bias),
            node_add_bias: dot(&self.weights.node_add_bias),
            edge_add_bias: dot(&self.weights.edge_add_bias),
            rewrite_bias: dot(&self.weights.rewrite_bias),
        }
    }
    pub fn decide(&self, features: &[f64]) -> ExecutionPolicyDecision {
        let dot = |w: &Vec<f64>| -> f64 {
            w.iter().zip(features.iter()).map(|(a, b)| a * b).sum()
        };
        let raw_run = dot(&self.weights.run_planner_head);
        let raw_expand = dot(&self.weights.expansion_head);
        let raw_exec = dot(&self.weights.execution_head);
        let raw_unblock = dot(&self.weights.unblock_head);
        let run_planner = raw_run >= 0.0;
        let expansion_scale = (1.0 + raw_expand).clamp(0.5, 2.0);
        let execution_preference = raw_exec.clamp(-1.0, 1.0);
        let prioritize_unblock = raw_unblock > 0.0;
        ExecutionPolicyDecision {
            run_planner,
            expansion_scale,
            prioritize_unblock,
            execution_preference,
        }
    }
    pub fn weight_norm(&self) -> f64 {
        let all = self
            .weights
            .planner_bias
            .iter()
            .chain(self.weights.node_add_bias.iter())
            .chain(self.weights.edge_add_bias.iter())
            .chain(self.weights.rewrite_bias.iter())
            .chain(self.weights.run_planner_head.iter())
            .chain(self.weights.expansion_head.iter())
            .chain(self.weights.execution_head.iter())
            .chain(self.weights.unblock_head.iter());
        all.map(|v| v * v).sum::<f64>().sqrt()
    }
}
fn policy_model_default_weights() -> ExecutionPolicyWeights {
    ExecutionPolicyWeights {
        planner_bias: vec![0.2, 0.0, 0.0, 0.0],
        node_add_bias: vec![0.2, 0.0, 0.0, 0.0],
        edge_add_bias: vec![0.1, 0.0, 0.0, 0.0],
        rewrite_bias: vec![0.1, 0.0, 0.0, 0.0],
        run_planner_head: vec![0.3, 0.0, 0.0, 0.0],
        expansion_head: vec![0.2, 0.0, 0.0, 0.0],
        execution_head: vec![0.8, 0.0, 0.0, 0.0],
        unblock_head: vec![0.2, 0.0, 0.0, 0.0],
    }
}
pub fn policy_model_format_bias(bias: &PolicyModelPolicyBias) -> String {
    if bias.planner_bias == 0.0 && bias.node_add_bias == 0.0 && bias.edge_add_bias == 0.0
        && bias.rewrite_bias == 0.0
    {
        return String::new();
    }
    format!(
        "Policy bias:\nplanner_bias={:.3}\nnode_add_bias={:.3}\nedge_add_bias={:.3}\nrewrite_bias={:.3}\n\
Prefer actions with positive bias and avoid strongly negative bias.\n",
        bias.planner_bias, bias.node_add_bias, bias.edge_add_bias, bias.rewrite_bias
    )
}
pub fn policy_model_smooth_bias(
    prev: Option<&PolicyModelPolicyBias>,
    next: PolicyModelPolicyBias,
) -> PolicyModelPolicyBias {
    if let Some(p) = prev {
        PolicyModelPolicyBias {
            planner_bias: 0.8 * p.planner_bias + 0.2 * next.planner_bias,
            node_add_bias: 0.8 * p.node_add_bias + 0.2 * next.node_add_bias,
            edge_add_bias: 0.8 * p.edge_add_bias + 0.2 * next.edge_add_bias,
            rewrite_bias: 0.8 * p.rewrite_bias + 0.2 * next.rewrite_bias,
        }
    } else {
        next
    }
}
pub fn policy_model_maybe_explore(
    mut bias: PolicyModelPolicyBias,
    epsilon: f64,
) -> PolicyModelPolicyBias {
    let mut rng = rand::thread_rng();
    if rng.r#gen::<f64>() < epsilon {
        bias.planner_bias = 0.0;
        bias.node_add_bias = 0.0;
        bias.edge_add_bias = 0.0;
        bias.rewrite_bias = 0.0;
    }
    bias
}
