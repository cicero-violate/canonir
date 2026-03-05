use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const CACHE_PATH: &str = "/workspace/ai_sandbox/canon/agent_logs/goal_embeddings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEmbedding {
    pub vector: Vec<f32>,
}

pub fn embed_goal(goal: &str, dim: usize) -> GoalEmbedding {
    let dim = dim.max(8);
    let mut vec = vec![0.0f32; dim];
    for token in goal.split_whitespace() {
        let mut h = fnv64(token.as_bytes());
        let sign = if (h & 1) == 0 { 1.0 } else { -1.0 };
        let idx = (h as usize) % dim;
        vec[idx] += sign;
        h = h.wrapping_mul(0x9e3779b97f4a7c15);
        let idx2 = (h as usize) % dim;
        vec[idx2] += sign * 0.5;
    }
    GoalEmbedding { vector: vec }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += (*x as f64) * (*y as f64);
        na += (*x as f64) * (*x as f64);
        nb += (*y as f64) * (*y as f64);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        (dot / (na.sqrt() * nb.sqrt())).clamp(0.0, 1.0)
    }
}

pub fn load_cache() -> HashMap<String, Vec<f32>> {
    std::fs::read_to_string(CACHE_PATH).ok().and_then(|s| serde_json::from_str::<HashMap<String, Vec<f32>>>(&s).ok()).unwrap_or_default()
}

pub fn save_cache(cache: &HashMap<String, Vec<f32>>) {
    if let Ok(pretty) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::create_dir_all(Path::new(CACHE_PATH).parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(CACHE_PATH, pretty);
    }
}

pub fn goal_hash(goal: &str) -> String {
    format!("{:016x}", fnv64(goal.as_bytes()))
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
