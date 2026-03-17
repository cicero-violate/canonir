use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalType {
    ReduceBranching,
    RemoveDeadCode,
    MergePaths,
    BreakCycle,
    SimplifyCallgraph,
    BreakDeadlock,
    ReduceDepth,
    ImproveCompletionVelocity,
}

impl std::fmt::Display for GoalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            GoalType::ReduceBranching => "reduce_branching",
            GoalType::RemoveDeadCode => "remove_dead_code",
            GoalType::MergePaths => "merge_paths",
            GoalType::BreakCycle => "break_cycle",
            GoalType::SimplifyCallgraph => "simplify_callgraph",
            GoalType::BreakDeadlock => "break_deadlock",
            GoalType::ReduceDepth => "reduce_depth",
            GoalType::ImproveCompletionVelocity => "improve_completion_velocity",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalArtifact {
    pub target_symbols: Vec<String>,
    pub target_files: Vec<String>,
    pub objective_type: GoalType,
    pub success_criteria: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalSpec {
    pub raw: String,
    pub embedding: Vec<f32>,
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub artifact: Option<GoalArtifact>,
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
            artifact: None,
        }
    }

    pub fn from_file(path: &str, embedding_dim: usize) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read goal file: {}", path))?;
        Ok(Self::new(raw, embedding_dim))
    }

    pub fn new_with_artifact(
        raw: String,
        embedding_dim: usize,
        artifact: Option<GoalArtifact>,
    ) -> Self {
        let mut spec = Self::new(raw, embedding_dim);
        if let Some(artifact) = artifact {
            spec.success_criteria.push(artifact.success_criteria.clone());
            spec.success_criteria.push("objective_improved".into());
            spec.artifact = Some(artifact);
        }
        spec
    }
}
