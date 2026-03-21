use serde::Deserialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, RwLock};
//
const PROMPTS_DIR: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts";
const GOAL_PROMPT_FILE: &str = "AGENT_GOAL.md";
const AGENT_CONFIG_TOML: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/capability_config.toml";

// ---------------------------------------------------------------------------
// PromptRegistry — shared between bootstrap and LlmExecutorConsumer
// ---------------------------------------------------------------------------

pub struct PromptRegistry {
    /// prompt_id (stem without .md) → file content
    prompts: HashMap<String, String>,
}

impl PromptRegistry {
    /// Look up prompt content by id. Supports ids with or without ".md".
    pub fn get(&self, prompt_id: &str) -> Option<&str> {
        let key = prompt_id.strip_suffix(".md").unwrap_or(prompt_id);
        self.prompts.get(key).map(String::as_str)
    }
}

pub type PromptRegistryHandle = Arc<RwLock<PromptRegistry>>;

pub fn new_prompt_registry() -> PromptRegistryHandle {
    Arc::new(RwLock::new(PromptRegistry { prompts: HashMap::new() }))
}

// ---------------------------------------------------------------------------
// Agent card deserialization (agents.cards section of capability_config.toml)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AgentConfigRaw {
    #[serde(default)]
    agents: AgentCardsWrapper,
}

#[derive(Default, Deserialize)]
struct AgentCardsWrapper {
    #[serde(default)]
    cards: Vec<AgentCardRaw>,
}

#[derive(Deserialize)]
struct AgentCardRaw {
    agent_id: String,
    agent_url: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    tool_capabilities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Bootstrap entry point
// ---------------------------------------------------------------------------

/// Read config files and prompt assets, populate the registry, and write audit
/// events to the tlog. Safe to call on every startup — idempotent per content hash.
pub fn bootstrap_config(tlog_path: &Path, registry: &PromptRegistryHandle) {
    bootstrap_prompts(tlog_path, registry);
    bootstrap_agents(tlog_path);
}

// ---------------------------------------------------------------------------
// Prompt loading
// ---------------------------------------------------------------------------

fn content_hash(s: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn bootstrap_prompts(tlog_path: &Path, registry: &PromptRegistryHandle) {
    let goal_path = Path::new(PROMPTS_DIR).join(GOAL_PROMPT_FILE);
    if !goal_path.exists() {
        return;
    }
    let mut reg = match registry.write() {
        Ok(r) => r,
        Err(_) => return,
    };
    let content = match std::fs::read_to_string(&goal_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let stem = "AGENT_GOAL".to_string();
    let hash = content_hash(&content);
    reg.prompts.insert(stem.clone(), content.clone());
    let payload = serde_json::json!({
        "prompt_id": stem,
        "path": GOAL_PROMPT_FILE,
        "hash": hash,
        "content": content,
    });
    write_boot_event(tlog_path, "prompt_loaded", payload);
}

// ---------------------------------------------------------------------------
// Agent registration
// ---------------------------------------------------------------------------

fn bootstrap_agents(tlog_path: &Path) {
    let text = match std::fs::read_to_string(AGENT_CONFIG_TOML) {
        Ok(t) => t,
        Err(_) => return,
    };
    let config: AgentConfigRaw = match toml::from_str(&text) {
        Ok(c) => c,
        Err(_) => return,
    };
    for card in &config.agents.cards {
        let payload = serde_json::json!({
            "agent_id": card.agent_id,
            "agent_url": card.agent_url,
            "role": card.role,
            "goal": card.goal,
            "tool_capabilities": card.tool_capabilities,
        });
        write_boot_event(tlog_path, "agent_registered", payload);
    }
}

// ---------------------------------------------------------------------------
// Tlog writer helper — handles both binary dir and JSON file formats
// ---------------------------------------------------------------------------

fn write_boot_event(tlog_path: &Path, kind: &str, payload: serde_json::Value) {
    let _ = canon_meta::canon_emit_meta!("bootstrap", kind, payload, tlog_path);
}

// ---------------------------------------------------------------------------
// Hot-reload: re-read one prompt file, update registry, write tlog event.
// Called by the prompt-directory watcher (P4) at runtime.
// ---------------------------------------------------------------------------

pub fn prompts_dir() -> &'static str {
    PROMPTS_DIR
}

pub fn reload_prompt_file(path: &Path, tlog_path: &Path, registry: &PromptRegistryHandle) {
    if path.file_name().and_then(|s| s.to_str()) != Some(GOAL_PROMPT_FILE) {
        return;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let filename = match path.file_name().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return,
    };
    let hash = content_hash(&content);
    if let Ok(mut reg) = registry.write() {
        reg.prompts.insert(stem.clone(), content.clone());
    }
    let payload = serde_json::json!({
        "prompt_id": stem,
        "path": filename,
        "hash": hash,
        "content": content,
    });
    let _ = canon_meta::canon_emit_meta!("prompt-watcher", "prompt_loaded", payload, tlog_path);
}
