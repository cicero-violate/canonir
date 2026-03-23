use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Task {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 { 60 }

#[derive(Debug, Deserialize)]
pub struct TaskFile { pub task: Vec<Task> }

pub fn load(path: &str) -> Result<Vec<Task>, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: TaskFile = toml::from_str(&content).map_err(|e| e.to_string())?;
    validate(&parsed.task)?;
    Ok(parsed.task)
}

pub fn validate(tasks: &[Task]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for t in tasks {
        if !seen.insert(&t.name) {
            return Err(format!("duplicate task {}", t.name));
        }
    }
    let names: HashSet<_> = tasks.iter().map(|t| t.name.as_str()).collect();
    for t in tasks {
        for dep in &t.depends_on {
            if !names.contains(dep.as_str()) {
                return Err(format!("missing dep {} for {}", dep, t.name));
            }
        }
    }
    Ok(())
}

