use super::dag::ExecutionGraph;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSnapshot {
    pub graph: ExecutionGraph,
    pub iteration: u64,
}
pub fn snapshot_store_save(path: &Path, snapshot: &PipelineSnapshot) {
    if let Ok(pretty) = serde_json::to_string_pretty(snapshot) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, pretty);
    }
}
pub fn snapshot_store_load(path: &Path) -> Option<PipelineSnapshot> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}
