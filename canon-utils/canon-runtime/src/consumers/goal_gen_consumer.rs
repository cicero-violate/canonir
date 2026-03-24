use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, RuntimeEvent, LlmCall, CapabilityResult};
use std::path::PathBuf;
use uuid::Uuid;

const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
const GOALGEN_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects/goalgen";

// Prompt instructs planner LLM to generate a single Rust project goal in the canonical AGENT_GOAL.md format.
const GOAL_GEN_PROMPT: &str = r#"
You are a software engineering challenge generator for a multi-agent Rust coding system.

Generate a SINGLE complex Rust project specification in EXACTLY the format shown below.
Output ONLY the markdown — no preamble, no explanation, nothing else.

Rules:
- The project MUST be a Rust binary crate
- Must require 800+ lines of real implementation across multiple modules
- Must be self-contained — only crates.io dependencies, no workspace deps
- Target path MUST be under /workspace/ai_sandbox/canon/test_projects/goalgen/<slug>
- `cargo check` passing is the sole success criterion
- Choose a different project category each time (VM, parser, CLI tool, scheduler, graph lib, etc.)

OUTPUT FORMAT (replace <...> placeholders):

# <Project Title>

<One paragraph describing what the project does and why it is interesting.>

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/<slug>`

## Requirements

<numbered list of 8–12 specific, concrete implementation requirements>
"#;

enum State {
    Waiting,
    Pending(String), // request_id
    Done,
}

pub struct GoalGenConsumer {
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    state: State,
}

impl GoalGenConsumer {
    pub fn new(tlog_path: PathBuf) -> Self {
        Self { tlog_path, emitter: None, state: State::Waiting }
    }
}

impl EventConsumer for GoalGenConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        match (&self.state, event) {
            (State::Waiting, RuntimeEvent::Tick(_)) => {
                let Some(emitter) = &self.emitter else { return; };
                let request_id = Uuid::new_v4().to_string();
                emitter.emit(RuntimeEvent::Llm(LlmCall {
                    request_id: request_id.clone(),
                    prompt: GOAL_GEN_PROMPT.to_string(),
                    role: Some("goal_gen".to_string()),
                    agent_id: Some("goal_gen_chatgpt".to_string()),
                }));
                self.state = State::Pending(request_id);
            }
            (State::Pending(expected_id), RuntimeEvent::CapabilityCompleted(done)) => {
                if done.request_id != *expected_id || done.capability != "llm.call" {
                    return;
                }
                let content = match &done.result {
                    CapabilityResult::Llm(res) => {
                        let raw = res.response.get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| res.response.as_str().unwrap_or(""));
                        extract_goal_text(raw)
                    }
                    _ => String::new(),
                };
                if validate_goal(&content) {
                    let _ = std::fs::write(AGENT_GOAL_PATH, &content);
                    crate::bootstrap::write_prompt_loaded_to_tlog(&self.tlog_path, &content);
                    self.state = State::Done;
                } else {
                    // Invalid output — retry on next tick.
                    self.state = State::Waiting;
                }
            }
            _ => {}
        }
    }
}

fn extract_goal_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("```markdown") {
        if let Some(inner) = inner.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    if let Some(inner) = trimmed.strip_prefix("```") {
        if let Some(inner) = inner.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn validate_goal(content: &str) -> bool {
    content.contains(GOALGEN_PROJECTS_DIR) && content.contains("## Requirements")
}
