use serde::{Deserialize, Serialize};
use std::path::Path;

use super::dag::TaskGraph;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub graph: TaskGraph,
    pub iteration: u64,
}

pub fn save(path: &Path, snapshot: &StateSnapshot) {
    if let Ok(pretty) = serde_json::to_string_pretty(snapshot) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, pretty);
    }
}

pub fn load(path: &Path) -> Option<StateSnapshot> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}
