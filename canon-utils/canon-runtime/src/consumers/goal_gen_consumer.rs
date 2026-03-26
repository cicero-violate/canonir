use canon_event::{new_error_occurred, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, LlmCall, PromptLoaded, RuntimeEvent};
use canon_proc_macros::must_emit;
use canon_skills::global_registry;
use std::path::PathBuf;
use uuid::Uuid;

const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
const GOALGEN_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects/goalgen";

enum State {
    Waiting,
    Pending { request_id: String },
    Done,
}

const MAX_RETRIES: u32 = 5;

pub struct GoalGenConsumer {
    tlog_path: PathBuf,
    state: State,
    retries: u32,
    emitter: Option<EventEmitterHandle>,
}

impl GoalGenConsumer {
    pub fn new(tlog_path: PathBuf) -> Self {
        let initial_state = if let Ok(existing) = std::fs::read_to_string(AGENT_GOAL_PATH) {
            if validate_goal(&existing) {
                eprintln!("[goal_gen] valid goal already on disk, skipping generation");
                State::Done
            } else {
                State::Waiting
            }
        } else {
            State::Waiting
        };
        Self { tlog_path, state: initial_state, retries: 0, emitter: None }
    }
}

impl EventConsumer for GoalGenConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        match (&self.state, event) {
            (State::Waiting, RuntimeEvent::PromptLoaded(p)) => {
                let content = p.payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if !is_placeholder_goal(content) {
                    return EventOutcome::NoOp("goal_gen_real_goal_already_loaded");
                }
                let request_id = Uuid::new_v4().to_string();
                self.state = State::Pending { request_id: request_id.clone() };
                let prompt = match global_registry().load("goal_gen/generate_goal") {
                    Ok(skill) => skill.prompt.clone(),
                    Err(e) => {
                        eprintln!("[goal_gen] failed to load skill: {e}");
                        self.state = State::Done;
                        return EventOutcome::NoOp("goal_gen_skill_load_failed");
                    }
                };
                EventOutcome::Emit(RuntimeEvent::Llm(LlmCall { request_id: request_id.clone(), prompt, role: Some("goal_gen".to_string()), agent_id: Some("goal_gen_chatgpt".to_string()), dispatched: true, system: None, system_prompt_id: None, context_base: None, context_base_id: None, prompt_base_id: None, prev_prompt_id: None }))
            }
            (State::Pending { request_id: expected_id, .. }, RuntimeEvent::CapabilityCompleted(done)) => {
                if done.request_id != *expected_id || done.capability != "llm.call" {
                    return EventOutcome::NoOp("goal_gen_unrelated_completion");
                }
                let mut warn_events: Vec<RuntimeEvent> = Vec::new();
                let content = match &done.result {
                    CapabilityResult::Llm(res) => {
                        let raw: String = if let Some(s) = res.response.get("text").and_then(|v| v.as_str()) {
                            s.to_string()
                        } else if let Some(s) = res.response.as_str() {
                            s.to_string()
                        } else if let Some(obj) = res.response.as_object() {
                            obj.values().find_map(|v| v.as_str().filter(|s| !s.is_empty())).unwrap_or("").to_string()
                        } else {
                            String::new()
                        };
                        if raw.is_empty() {
                            eprintln!("[goal_gen] LLM returned empty response (response shape: {})", res.response.to_string().chars().take(200).collect::<String>());
                            warn_events.push(RuntimeEvent::ErrorOccurred(new_error_occurred(
                                "goal_gen_empty_response",
                                "goal_gen_consumer",
                                "LLM returned empty or unparseable content for goal generation",
                                "warning",
                                serde_json::json!({
                                    "retry": self.retries,
                                    "response_shape": res.response.to_string().chars().take(200).collect::<String>(),
                                }),
                                Some(done.request_id.clone()),
                            )));
                        }
                        extract_goal_text(&raw)
                    }
                    _ => String::new(),
                };
                if validate_goal(&content) {
                    let _ = std::fs::write(AGENT_GOAL_PATH, &content);
                    crate::bootstrap::write_prompt_loaded_to_tlog(&self.tlog_path, &content);
                    emit_prompt_loaded(&self.emitter, &content, &trigger_id);
                    self.state = State::Done;
                    if warn_events.is_empty() {
                        EventOutcome::NoOp("goal_gen_done")
                    } else {
                        EventOutcome::EmitMany(warn_events)
                    }
                } else {
                    self.retries += 1;
                    if self.retries >= MAX_RETRIES {
                        let msg = format!("goal_gen gave up after {MAX_RETRIES} retries — last content was {} bytes: {}", content.len(), &content[..content.len().min(200)]);
                        eprintln!("[goal_gen] {msg}");
                        self.state = State::Done;
                        return EventOutcome::Error(RuntimeEvent::ErrorOccurred(new_error_occurred(
                            "goal_gen_exhausted",
                            "goal_gen_consumer",
                            &msg,
                            "error",
                            serde_json::json!({ "retries": MAX_RETRIES, "content_bytes": content.len() }),
                            None,
                        )));
                    } else {
                        eprintln!("[goal_gen] retry {}/{}", self.retries, MAX_RETRIES);
                        self.state = State::Waiting;
                    }
                    if warn_events.is_empty() {
                        EventOutcome::NoOp("goal_gen_retrying")
                    } else {
                        EventOutcome::EmitMany(warn_events)
                    }
                }
            }
            (State::Pending { request_id: expected_id, .. }, RuntimeEvent::CapabilityFailed(fail)) => {
                if fail.request_id != *expected_id || fail.capability != "llm.call" {
                    return EventOutcome::NoOp("goal_gen_unrelated_failure");
                }
                self.retries += 1;
                let msg = format!("goal_gen LLM call failed: {}", fail.error);
                eprintln!("[goal_gen] {msg} (retry {}/{})", self.retries, MAX_RETRIES);
                if self.retries >= MAX_RETRIES {
                    eprintln!("[goal_gen] gave up after {MAX_RETRIES} retries due to capability failures");
                    self.state = State::Done;
                    return EventOutcome::Error(RuntimeEvent::ErrorOccurred(new_error_occurred(
                        "goal_gen_exhausted",
                        "goal_gen_consumer",
                        &msg,
                        "error",
                        serde_json::json!({ "retries": MAX_RETRIES, "last_error": fail.error }),
                        None,
                    )));
                } else {
                    self.state = State::Waiting;
                }
                EventOutcome::NoOp("goal_gen_failure_retry")
            }
            (State::Waiting, RuntimeEvent::CapabilityCompleted(_)) | (State::Waiting, RuntimeEvent::CapabilityFailed(_)) => EventOutcome::NoOp("goal_gen_waiting_unrelated"),
            (State::Done, _) => EventOutcome::NoOp("goal_gen_noop"),
            (_, RuntimeEvent::Tick(_))
            | (_, RuntimeEvent::Code(_))
            | (_, RuntimeEvent::Debug(_))
            | (_, RuntimeEvent::Edit(_))
            | (_, RuntimeEvent::ErrorOccurred(_))
            | (_, RuntimeEvent::LoopObserved(_))
            | (_, RuntimeEvent::LoopPlanned(_))
            | (_, RuntimeEvent::LoopActed(_))
            | (_, RuntimeEvent::LoopVerified(_))
            | (_, RuntimeEvent::LoopRewarded(_))
            | (_, RuntimeEvent::GoodnessSnapshot(_))
            | (_, RuntimeEvent::RouteTick(_))
            | (_, RuntimeEvent::RouteSelected(_))
            | (_, RuntimeEvent::Cargo(_))
            | (_, RuntimeEvent::File(_))
            | (_, RuntimeEvent::Bash(_))
            | (_, RuntimeEvent::Llm(_))
            | (_, RuntimeEvent::RequestDispatch(_))
            | (_, RuntimeEvent::SubTaskResult(_))
            | (_, RuntimeEvent::Analysis(_))
            | (_, RuntimeEvent::RuntimeStateUpdated(_))
            | (_, RuntimeEvent::NodeReady(_))
            | (_, RuntimeEvent::NodeStarted(_))
            | (_, RuntimeEvent::NodeCompleted(_))
            | (_, RuntimeEvent::NodeFailed(_))
            | (_, RuntimeEvent::PolicyBaselineUpdated(_))
            | (_, RuntimeEvent::GoalSelected(_))
            | (_, RuntimeEvent::SystemConfigLoaded(_))
            | (_, RuntimeEvent::AgentRegistered(_))
            | (_, RuntimeEvent::PromptLoaded(_))
            | (_, RuntimeEvent::ToolCall(_))
            | (_, RuntimeEvent::ToolResult(_))
            | (_, RuntimeEvent::ToolBatchSettled(_))
            | (_, RuntimeEvent::GoalNodeCreated(_))
            | (_, RuntimeEvent::GoalNodeRetracted(_))
            | (_, RuntimeEvent::GoalNodeRewritten(_))
            | (_, RuntimeEvent::GoalEdgeDefined(_))
            | (_, RuntimeEvent::GoalGraphCheckpointed(_))
            | (_, RuntimeEvent::CapabilityInvoked(_))
            | (_, RuntimeEvent::CapabilityResolved(_))
            | (_, RuntimeEvent::InvariantDiscovered(_)) => EventOutcome::NoOp("goal_gen_noop"),
        }
    }
}

fn is_placeholder_goal(goal: &str) -> bool {
    let trimmed = goal.trim();
    trimmed.is_empty() || trimmed.contains("goal-pending")
}

fn extract_goal_text(raw: &str) -> String {
    let trimmed = raw.trim();

    // If there is a fenced code block anywhere, extract its content.
    for fence_label in &["```markdown\n", "```md\n", "```\n", "``` \n"] {
        if let Some(start) = trimmed.find(fence_label) {
            let after_fence = &trimmed[start + fence_label.len()..];
            if let Some(end) = after_fence.find("\n```") {
                let inner = after_fence[..end].trim();
                if !inner.is_empty() {
                    return inner.to_string();
                }
            }
        }
    }

    // No fence — strip any leading prose before the first heading.
    if let Some(heading_start) = if trimmed.starts_with("# ") { Some(0) } else { trimmed.find("\n# ") } {
        return trimmed[heading_start..].trim_start().to_string();
    }

    trimmed.to_string()
}

fn validate_goal(content: &str) -> bool {
    let has_path = content.contains(GOALGEN_PROJECTS_DIR);
    let lower = content.to_lowercase();
    let has_requirements = lower.contains("## requirements");
    let has_heading = content.trim_start().starts_with('#');

    let ok = has_path && has_requirements && has_heading;
    if !ok {
        eprintln!("[goal_gen] validation failed: has_path={has_path} has_requirements={has_requirements} has_heading={has_heading} | preview: {:?}", &content[..content.len().min(300)]);
    }
    ok
}


fn emit_prompt_loaded(emitter: &Option<EventEmitterHandle>, content: &str, trigger_id: &EventId) {
    if let Some(em) = emitter {
        let event = RuntimeEvent::PromptLoaded(PromptLoaded {
            payload: serde_json::json!({
                "prompt_id": "AGENT_GOAL",
                "path": AGENT_GOAL_PATH,
                "content": content,
            }),
        });
        em.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
    }
}
