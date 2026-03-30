use canon_prompt_events::{prompt_file_id, prompt_loaded_payload, GOAL_PROMPT_FILE, GOAL_PROMPT_ID, PROMPTS_DIR};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
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

/// Read config files and prompt assets and populate the registry.
/// PromptLoaded emission is authoritative through the runtime bus only.
pub fn bootstrap_config(tlog_path: &Path, registry: &PromptRegistryHandle) {
    bootstrap_prompts(registry);
    let _ = tlog_path;
}

// ---------------------------------------------------------------------------
// Prompt loading
// ---------------------------------------------------------------------------

fn bootstrap_prompts(registry: &PromptRegistryHandle) {
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
    reg.prompts.insert(GOAL_PROMPT_ID.to_string(), content);
}

/// Returns the agent cards from capability_config.toml as JSON values.
/// Used by event_runtime.rs to emit AgentRegistered events to the live bus at startup.
pub fn load_agent_cards() -> Vec<serde_json::Value> {
    let text = match std::fs::read_to_string(AGENT_CONFIG_TOML) {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    let config: AgentConfigRaw = match toml::from_str(&text) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    config
        .agents
        .cards
        .iter()
        .map(|card| {
            serde_json::json!({
                "agent_id": card.agent_id,
                "agent_url": card.agent_url,
                "role": card.role,
                "goal": card.goal,
                "tool_capabilities": card.tool_capabilities,
                "capacity": 1,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hot-reload: re-read one prompt file and update the registry. The caller is
// responsible for emitting the authoritative PromptLoaded runtime event.
// ---------------------------------------------------------------------------

pub fn prompts_dir() -> &'static str {
    PROMPTS_DIR
}

pub fn reload_prompt_file(path: &Path, registry: &PromptRegistryHandle) -> Option<canon_event::PromptLoaded> {
    if path.file_name().and_then(|s| s.to_str()) != Some(GOAL_PROMPT_FILE) {
        return None;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let prompt_id = prompt_file_id(path)?;
    if let Ok(mut reg) = registry.write() {
        reg.prompts.insert(prompt_id.clone(), content.clone());
    }
    Some(canon_event::PromptLoaded { payload: prompt_loaded_payload(&prompt_id, &path.display().to_string(), &content) })
}
