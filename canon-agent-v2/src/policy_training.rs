use serde::{Deserialize, Serialize};
use std::path::Path;
use super::graph_algo::{graph_analysis_normalize_features, GraphFeatureVector};
use super::policy::{ExecutionPolicyModel, ExecutionPolicyWeights};
const DATASET_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl";
const WEIGHTS_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/policy_weights.json";
const LEARNING_RATE: f64 = 0.01;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTrainingPolicyDatasetEntry {
    pub features: serde_json::Value,
    pub action: serde_json::Value,
    #[serde(default)]
    pub policy_decision: serde_json::Value,
    pub reward: f64,
}
pub fn policy_training_load_dataset() -> Vec<PolicyTrainingPolicyDatasetEntry> {
    let text = std::fs::read_to_string(DATASET_PATH).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            serde_json::from_str::<PolicyTrainingPolicyDatasetEntry>(line).ok()
        })
        .collect()
}
pub fn policy_training_dataset_size() -> u64 {
    std::fs::read_to_string(DATASET_PATH)
        .ok()
        .map(|s| s.lines().count() as u64)
        .unwrap_or(0)
}
fn policy_training_features_from_json(
    value: &serde_json::Value,
) -> Option<GraphFeatureVector> {
    let mut clean = value.clone();
    if let serde_json::Value::Object(map) = &mut clean {
        map.remove("failures");
    }
    serde_json::from_value::<GraphFeatureVector>(clean).ok()
}
pub fn train_policy_weights(
    max_nodes: usize,
    max_edges: usize,
) -> ExecutionPolicyWeights {
    let entries = policy_training_load_dataset();
    let mut weights = ExecutionPolicyWeights {
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
        if let Some(features) = policy_training_features_from_json(&entry.features) {
            let fv = graph_analysis_normalize_features(&features, max_nodes, max_edges);
            policy_training_ensure_head_len(&mut weights, fv.len());
            policy_training_update_head(&mut weights.planner_bias, &fv, entry.reward);
            policy_training_update_head(
                &mut weights.run_planner_head,
                &fv,
                entry.reward,
            );
            policy_training_update_head(&mut weights.expansion_head, &fv, entry.reward);
            policy_training_update_head(&mut weights.execution_head, &fv, entry.reward);
            policy_training_update_head(&mut weights.unblock_head, &fv, entry.reward);
        }
    }
    weights
}
pub fn policy_training_save_weights(weights: &ExecutionPolicyWeights) {
    if let Ok(pretty) = serde_json::to_string_pretty(weights) {
        let _ = std::fs::write(WEIGHTS_PATH, pretty);
    }
}
pub fn policy_training_update_online(
    entry: &PolicyTrainingPolicyDatasetEntry,
    max_nodes: usize,
    max_edges: usize,
) {
    let mut model = ExecutionPolicyModel::load_default();
    let mut weights = model.weights.clone();
    if let Some(features) = policy_training_features_from_json(&entry.features) {
        let fv = graph_analysis_normalize_features(&features, max_nodes, max_edges);
        policy_training_ensure_head_len(&mut weights, fv.len());
        policy_training_update_head(&mut weights.planner_bias, &fv, entry.reward);
        policy_training_update_head(&mut weights.run_planner_head, &fv, entry.reward);
        policy_training_update_head(&mut weights.expansion_head, &fv, entry.reward);
        policy_training_update_head(&mut weights.execution_head, &fv, entry.reward);
        policy_training_update_head(&mut weights.unblock_head, &fv, entry.reward);
        model = ExecutionPolicyModel { weights };
        let _ = model.snapshot_store_save(Path::new(WEIGHTS_PATH));
    }
}
pub fn policy_training_append_policy_dataset(entry: &PolicyTrainingPolicyDatasetEntry) {
    let path = Path::new(DATASET_PATH);
    if let Ok(line) = serde_json::to_string(entry) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(
                &mut f,
                format!("{}\n", line).as_bytes(),
            ));
    }
}
fn policy_training_dot(w: &[f64], f: &[f64]) -> f64 {
    w.iter().zip(f.iter()).map(|(a, b)| a * b).sum()
}
fn policy_training_ensure_head_len(weights: &mut ExecutionPolicyWeights, len: usize) {
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
fn policy_training_update_head(head: &mut [f64], fv: &[f64], reward: f64) {
    let predicted = policy_training_dot(head, fv);
    let error = reward - predicted;
    for (w, f) in head.iter_mut().zip(fv.iter()) {
        *w += LEARNING_RATE * error * f;
    }
}
