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
    #[serde(default)]
    pub policy_decision: serde_json::Value,
    pub reward: f64,
}

pub fn load_dataset() -> Vec<PolicyDatasetEntry> {
    let text = std::fs::read_to_string(DATASET_PATH).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str::<PolicyDatasetEntry>(line).ok()).collect()
}

pub fn dataset_size() -> u64 {
    std::fs::read_to_string(DATASET_PATH).ok().map(|s| s.lines().count() as u64).unwrap_or(0)
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
        run_planner_head: Vec::new(),
        expansion_head: Vec::new(),
        execution_head: Vec::new(),
        unblock_head: Vec::new(),
    };
    for entry in entries {
        if let Some(features) = features_from_json(&entry.features) {
            let fv = normalize_features(&features, max_nodes, max_edges);
            ensure_head_len(&mut weights, fv.len());
            update_head(&mut weights.planner_bias, &fv, entry.reward);
            update_head(&mut weights.run_planner_head, &fv, entry.reward);
            update_head(&mut weights.expansion_head, &fv, entry.reward);
            update_head(&mut weights.execution_head, &fv, entry.reward);
            update_head(&mut weights.unblock_head, &fv, entry.reward);
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
        ensure_head_len(&mut weights, fv.len());
        update_head(&mut weights.planner_bias, &fv, entry.reward);
        update_head(&mut weights.run_planner_head, &fv, entry.reward);
        update_head(&mut weights.expansion_head, &fv, entry.reward);
        update_head(&mut weights.execution_head, &fv, entry.reward);
        update_head(&mut weights.unblock_head, &fv, entry.reward);
        model = PolicyModel { weights };
        let _ = model.save(Path::new(WEIGHTS_PATH));
    }
}

pub fn append_policy_dataset(entry: &PolicyDatasetEntry) {
    let path = Path::new(DATASET_PATH);
    if let Ok(line) = serde_json::to_string(entry) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| std::io::Write::write_all(&mut f, format!("{}\n", line).as_bytes()));
    }
}

fn dot(w: &[f64], f: &[f64]) -> f64 {
    w.iter().zip(f.iter()).map(|(a, b)| a * b).sum()
}

fn ensure_head_len(weights: &mut PolicyWeights, len: usize) {
    if weights.planner_bias.is_empty() {
        weights.planner_bias = vec![0.0; len];
    }
    if weights.run_planner_head.is_empty() {
        weights.run_planner_head = vec![0.0; len];
    }
    if weights.expansion_head.is_empty() {
        weights.expansion_head = vec![0.0; len];
    }
    if weights.execution_head.is_empty() {
        weights.execution_head = vec![0.0; len];
    }
    if weights.unblock_head.is_empty() {
        weights.unblock_head = vec![0.0; len];
    }
}

fn update_head(head: &mut [f64], fv: &[f64], reward: f64) {
    let predicted = dot(head, fv);
    let error = reward - predicted;
    for (w, f) in head.iter_mut().zip(fv.iter()) {
        *w += LEARNING_RATE * error * f;
    }
}
