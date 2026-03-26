use canon_event::{PromptLoaded, RuntimeEvent};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const PROMPTS_DIR: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts";
pub const GOAL_PROMPT_ID: &str = "AGENT_GOAL";
pub const GOAL_PROMPT_FILE: &str = "AGENT_GOAL.md";
pub const GOAL_PROMPT_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";

pub fn prompt_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn prompt_loaded_payload(prompt_id: &str, path: &str, content: &str) -> serde_json::Value {
    serde_json::json!({
        "prompt_id": prompt_id,
        "path": path,
        "hash": prompt_hash(content),
        "content": content,
    })
}

pub fn goal_prompt_loaded_payload(content: &str) -> serde_json::Value {
    prompt_loaded_payload(GOAL_PROMPT_ID, GOAL_PROMPT_PATH, content)
}

pub fn prompt_loaded_event(prompt_id: &str, path: &str, content: &str) -> PromptLoaded {
    PromptLoaded { payload: prompt_loaded_payload(prompt_id, path, content) }
}

pub fn goal_prompt_loaded_event(content: &str) -> PromptLoaded {
    PromptLoaded { payload: goal_prompt_loaded_payload(content) }
}

pub fn runtime_prompt_loaded(prompt_id: &str, path: &str, content: &str) -> RuntimeEvent {
    RuntimeEvent::PromptLoaded(prompt_loaded_event(prompt_id, path, content))
}

pub fn runtime_goal_prompt_loaded(content: &str) -> RuntimeEvent {
    RuntimeEvent::PromptLoaded(goal_prompt_loaded_event(content))
}

pub fn prompt_file_id(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(ToOwned::to_owned)
}
