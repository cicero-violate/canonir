use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildEvent {
    WorkspaceChanged { crate_name: String },
    BuildStarted { crate_name: String },
    BuildCompleted { crate_name: String, success: bool, duration_ms: u128 },
    CheckStarted { crate_name: String },
    CheckCompleted { crate_name: String, success: bool, duration_ms: u128 },
    RunStarted { crate_name: String, bin: Option<String> },
    RunCompleted { crate_name: String, bin: Option<String>, success: bool, duration_ms: u128 },
}
