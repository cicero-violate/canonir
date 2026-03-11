use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalSpec {
    pub raw: String,
    pub embedding: Vec<f32>,
    pub success_criteria: Vec<String>,
}

impl GoalSpec {
    pub fn new(raw: String, embedding_dim: usize) -> Self {
        let embedding =
            crate::goal_embedding::goal_embedding_embed_goal(&raw, embedding_dim).vector;
        Self {
            raw,
            embedding,
            success_criteria: vec![
                "graph_completed".into(),
                "no_failed_nodes".into(),
                "invariants_hold".into(),
            ],
        }
    }
}
