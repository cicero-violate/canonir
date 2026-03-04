use serde::{Deserialize, Serialize};
use std::path::Path;

use super::graph_algo::{normalize_features, FeatureVector};
use super::policy::{PolicyModel, PolicyWeights};

const DATASET_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl";
const WEIGHTS_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/policy_weights.json";
const LEARNING_RATE: f64 = 0.01;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDatasetEntry {
    pub features: serde_json::Value,
    pub action: serde_json::Value,
    pub reward: f64,
}

pub fn load_dataset() -> Vec<PolicyDatasetEntry> {
    let text = std::fs::read_to_string(DATASET_PATH).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<PolicyDatasetEntry>(line).ok())
        .collect()
}

pub fn dataset_size() -> u64 {
    std::fs::read_to_string(DATASET_PATH)
        .ok()
        .map(|s| s.lines().count() as u64)
        .unwrap_or(0)
}

fn features_from_json(value: &serde_json::Value) -> Option<FeatureVector> {
    let mut clean = value.clone();
    if let serde_json::Value::Object(map) = &mut clean {
        map.remove("failures");
    }
    serde_json::from_value::<FeatureVector>(clean).ok()
}

pub fn train_policy(max_nodes: usize, max_edges: usize) -> PolicyWeights {
    let entries = load_dataset();
    let mut weights = PolicyWeights {
        planner_bias: Vec::new(),
        node_add_bias: Vec::new(),
        edge_add_bias: Vec::new(),
        rewrite_bias: Vec::new(),
    };
    for entry in entries {
        if let Some(features) = features_from_json(&entry.features) {
            let fv = normalize_features(&features, max_nodes, max_edges);
            if weights.planner_bias.is_empty() {
                weights.planner_bias = vec![0.0; fv.len()];
            }
            let predicted = dot(&weights.planner_bias, &fv);
            let error = entry.reward - predicted;
            for (w, f) in weights.planner_bias.iter_mut().zip(fv.iter()) {
                *w += LEARNING_RATE * error * f;
            }
        }
    }
    weights
}

pub fn save_weights(weights: &PolicyWeights) {
    if let Ok(pretty) = serde_json::to_string_pretty(weights) {
        let _ = std::fs::write(WEIGHTS_PATH, pretty);
    }
}

pub fn update_online(entry: &PolicyDatasetEntry, max_nodes: usize, max_edges: usize) {
    let mut model = PolicyModel::load_default();
    let mut weights = model.weights.clone();
    if let Some(features) = features_from_json(&entry.features) {
        let fv = normalize_features(&features, max_nodes, max_edges);
        if weights.planner_bias.is_empty() {
            weights.planner_bias = vec![0.0; fv.len()];
        }
        let predicted = dot(&weights.planner_bias, &fv);
        let error = entry.reward - predicted;
        for (w, f) in weights.planner_bias.iter_mut().zip(fv.iter()) {
            *w += LEARNING_RATE * error * f;
        }
        model = PolicyModel { weights };
        let _ = model.save(Path::new(WEIGHTS_PATH));
    }
}

fn dot(w: &[f64], f: &[f64]) -> f64 {
    w.iter().zip(f.iter()).map(|(a, b)| a * b).sum()
}
