use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalSpec {
    pub schema_version: u32,
    pub id: String,
    pub goal_type: String,
    pub target_path: Option<PathBuf>,
    #[serde(default)]
    pub description: String,
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
            description: String::new(),
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
    let mut in_description = false;
    let mut description_lines = Vec::new();
    for line in goal_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            in_description = true;
            continue;
        }
        if trimmed.starts_with("## ") {
            in_description = false;
        }
        if in_description && !trimmed.is_empty() {
            description_lines.push(trimmed);
        }
        if let Some(path) = trimmed.strip_prefix("- Project path:") {
            let candidate = path.trim().trim_matches('`');
            if !candidate.is_empty() {
                spec.target_path = Some(PathBuf::from(candidate));
            }
            continue;
        }
        if trimmed.starts_with("- ") {
            spec.requirements.push(trimmed.trim_start_matches("- ").trim().to_string());
        }
    }
    spec.description = description_lines.join(" ");
    spec
}

pub fn summarize_goal(spec: &GoalSpec) -> String {
    let desc = if spec.description.is_empty() { "(no description)".to_string() } else { spec.description.clone() };
    let target = spec.target_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(unspecified)".to_string());
    let requirements = if spec.requirements.is_empty() { "(none)".to_string() } else { spec.requirements.iter().take(6).map(|r| format!("- {r}")).collect::<Vec<_>>().join("\n") };
    format!("{desc}\ntarget: {target}\nrequirements:\n{requirements}")
}
