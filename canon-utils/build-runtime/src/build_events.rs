use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildEvent {
    WorkspaceChanged { crate_name: String },
    BuildStarted { crate_name: String },
    BuildCompleted { crate_name: String, success: bool, duration_ms: u128 },
}
