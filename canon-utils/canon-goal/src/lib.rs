use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalSpec {
    pub schema_version: u32,
    pub id: String,
    pub goal_type: String,
    pub target_path: Option<PathBuf>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl Default for GoalSpec {
    fn default() -> Self {
        Self {
            schema_version: 1,
            id: "agent_goal".to_string(),
            goal_type: "workspace_task".to_string(),
            target_path: None,
            constraints: Vec::new(),
            requirements: Vec::new(),
            metadata: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Pending,
    Running,
    Satisfied,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalProgress {
    pub requirement: String,
    pub passed: bool,
    pub evidence: String,
}

pub fn parse_agent_goal_markdown(goal_text: &str) -> GoalSpec {
    let mut spec = GoalSpec::default();
    for line in goal_text.lines() {
        let trimmed = line.trim();
        if let Some(path) = trimmed.strip_prefix("- Project path:") {
            let candidate = path.trim().trim_matches('`');
            if !candidate.is_empty() {
                spec.target_path = Some(PathBuf::from(candidate));
            }
            continue;
        }
        if trimmed.starts_with("- ") {
            spec.requirements
                .push(trimmed.trim_start_matches("- ").trim().to_string());
        }
    }
    spec
}

pub fn summarize_goal(spec: &GoalSpec) -> String {
    let target = spec
        .target_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unspecified)".to_string());
    let requirements = if spec.requirements.is_empty() {
        "(none)".to_string()
    } else {
        spec.requirements
            .iter()
            .take(6)
            .map(|r| format!("- {r}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "goal_id={}\ngoal_type={}\ntarget_path={}\nrequirements:\n{}",
        spec.id, spec.goal_type, target, requirements
    )
}

