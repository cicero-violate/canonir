use canon_event::{new_error_occurred, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, LlmCall, RuntimeEvent};
use canon_prompt_events::goal_prompt_loaded_event;
use canon_proc_macros::must_emit;
use canon_semantic_state::{
    derive_self_development_objective_state, primary_development_objective_kind,
    primary_development_strategy_kind, DevelopmentObjectiveKind, DevelopmentStrategyKind,
    LlmSemanticContext, ObjectiveTrendState, SemanticStateSummary,
};
use canon_skills::global_registry;
use uuid::Uuid;
use crate::consumers::harness_repair_mode::harness_repair_mode_enabled;

const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
const GOALGEN_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects/goalgen";

enum State {
    Waiting,
    Pending { request_id: String },
    Done,
}

const MAX_RETRIES: u32 = 5;

pub struct GoalGenConsumer {
    state: State,
    retries: u32,
    emitter: Option<EventEmitterHandle>,
    semantic_summary: SemanticStateSummary,
    objective_trend_state: ObjectiveTrendState,
    last_route_objective: Option<DevelopmentObjectiveKind>,
}

impl GoalGenConsumer {
    pub fn new(_tlog_path: std::path::PathBuf) -> Self {
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
        Self {
            state: initial_state,
            retries: 0,
            emitter: None,
            semantic_summary: SemanticStateSummary::default(),
            objective_trend_state: ObjectiveTrendState::default(),
            last_route_objective: None,
        }
    }
}

impl EventConsumer for GoalGenConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "goal_gen_consumer" }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        if harness_repair_mode_enabled() {
            return EventOutcome::NoOp("goal_gen_suppressed_for_harness_repair");
        }
        match (&self.state, event) {
            (State::Waiting, RuntimeEvent::PromptLoaded(p)) => {
                let content = p.payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if !is_placeholder_goal(content) {
                    return EventOutcome::NoOp("goal_gen_real_goal_already_loaded");
                }
                let request_id = Uuid::new_v4().to_string();
                self.state = State::Pending { request_id: request_id.clone() };
                let semantic_context = LlmSemanticContext {
                    mission_summary: None,
                    semantic_summary: self.semantic_summary.clone(),
                    objective_state: derive_self_development_objective_state(
                        &self.semantic_summary,
                        0,
                        &[],
                        &self.objective_trend_state,
                    ),
                    objective_trend_state: self.objective_trend_state.clone(),
                    target_workspace: Some(GOALGEN_PROJECTS_DIR.to_string()),
                    workspace_loc: None,
                    error_count: None,
                    warning_count: None,
                    route_rationale: None,
                    route_confidence: None,
                    invalid_plan_reason: None,
                    invalid_plan_planned_count: None,
                    consecutive_invalid_plan_batches: 0,
                    low_level_diagnostics: vec![format!("goalgen_projects_dir={GOALGEN_PROJECTS_DIR}")],
                    recent_actions: Vec::new(),
                    recent_tool_results: Vec::new(),
                    recent_execution_results: Vec::new(),
                };
                let current_goal_objective = infer_goal_objective(content);
                let objective_override = self
                    .last_route_objective
                    .filter(|route_objective| goal_objective_drift(current_goal_objective, *route_objective))
                    .map(|route_objective| {
                        format!(
                            "\n\nGoal objective override:\n- The current runtime objective has drifted.\n- Choose the typed development objective `{}` first.\n- Rewrite the goal so it prioritizes: {}",
                            route_objective.as_str(),
                            route_objective.focus_text()
                        )
                    })
                    .unwrap_or_default();
                let selected_goal_objective = current_primary_objective(
                    &self.semantic_summary,
                    &self.objective_trend_state,
                );
                let selected_goal_strategy = current_primary_strategy(
                    &self.semantic_summary,
                    &self.objective_trend_state,
                );
                let prompt = match goal_gen_prompt(selected_goal_objective, selected_goal_strategy) {
                    Ok(prompt) => prompt,
                    Err(e) => {
                        eprintln!("[goal_gen] failed to load skill: {e}");
                        self.state = State::Done;
                        return EventOutcome::NoOp("goal_gen_skill_load_failed");
                    }
                };
                if !objective_override.is_empty() {
                    if let Some(emitter) = &self.emitter {
                        emitter.emit_with_parents(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "goal_gen_consumer".to_string(),
                                kind: "goal_objective_drift".to_string(),
                                payload: serde_json::json!({
                                    "goal_objective": current_goal_objective.map(DevelopmentObjectiveKind::as_str),
                                    "route_objective": self.last_route_objective.map(DevelopmentObjectiveKind::as_str),
                                }),
                            }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
                EventOutcome::emit(RuntimeEvent::Llm(LlmCall {
                    request_id: request_id.clone(),
                    prompt: format!(
                        "{prompt}\n\n{}\n\nGoal generation target objective:\n- {}\n- {}\nGoal generation target strategy:\n- {}\n- {}\n{}",
                        semantic_context.render_goal_gen_block(),
                        selected_goal_objective.as_str(),
                        selected_goal_objective.focus_text(),
                        selected_goal_strategy.as_str(),
                        selected_goal_strategy.focus_text(),
                        objective_override
                    ),
                    role: Some("goal_gen".to_string()),
                    agent_id: Some("goal_gen_chatgpt".to_string()),
                    dispatched: true,
                    system: None,
                    system_prompt_id: None,
                    context_base: None,
                    context_base_id: None,
                    prompt_base_id: None,
                    prev_prompt_id: None,
                }), file!(), line!())
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
                    emit_prompt_loaded(&self.emitter, &content, &trigger_id);
                    self.state = State::Done;
                    if warn_events.is_empty() {
                        EventOutcome::NoOp("goal_gen_done")
                    } else {
                        EventOutcome::emit_many(warn_events, file!(), line!())
                    }
                } else {
                    self.retries += 1;
                    if self.retries >= MAX_RETRIES {
                        let msg = format!("goal_gen gave up after {MAX_RETRIES} retries — last content was {} bytes: {}", content.len(), &content[..content.len().min(200)]);
                        eprintln!("[goal_gen] {msg}");
                        self.state = State::Done;
                        return EventOutcome::error(RuntimeEvent::ErrorOccurred(new_error_occurred(
                            "goal_gen_exhausted",
                            "goal_gen_consumer",
                            &msg,
                            "error",
                            serde_json::json!({ "retries": MAX_RETRIES, "content_bytes": content.len() }),
                            None,
                        )), file!(), line!());
                    } else {
                        eprintln!("[goal_gen] retry {}/{}", self.retries, MAX_RETRIES);
                        self.state = State::Waiting;
                    }
                    if warn_events.is_empty() {
                        EventOutcome::NoOp("goal_gen_retrying")
                    } else {
                        EventOutcome::emit_many(warn_events, file!(), line!())
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
                    return EventOutcome::error(RuntimeEvent::ErrorOccurred(new_error_occurred(
                        "goal_gen_exhausted",
                        "goal_gen_consumer",
                        &msg,
                        "error",
                        serde_json::json!({ "retries": MAX_RETRIES, "last_error": fail.error }),
                        None,
                    )), file!(), line!());
                } else {
                    self.state = State::Waiting;
                }
                EventOutcome::NoOp("goal_gen_failure_retry")
            }
            (_, RuntimeEvent::LoopObserved(observed)) => {
                self.semantic_summary = observed.semantic_summary.clone();
                self.objective_trend_state
                    .record_observation(observed.error_count, &self.semantic_summary);
                EventOutcome::NoOp("goal_gen_observed_update")
            }
            (_, RuntimeEvent::RouteSelected(_)) => {
                self.last_route_objective = Some(current_primary_objective(
                    &self.semantic_summary,
                    &self.objective_trend_state,
                ));
                EventOutcome::NoOp("goal_gen_route_objective_update")
            }
            (_, RuntimeEvent::PlanningCompleted(pc)) => {
                self.objective_trend_state.record_planning_completion(&pc.status);
                EventOutcome::NoOp("goal_gen_planning_update")
            }
            (_, RuntimeEvent::GoodnessSnapshot(g)) => {
                self.objective_trend_state.record_goodness(g.g, g.delta_g);
                EventOutcome::NoOp("goal_gen_goodness_update")
            }
            (_, RuntimeEvent::ErrorOccurred(err)) if err.kind == "invalid_plan_batch" => {
                self.objective_trend_state.record_invalid_plan_event();
                EventOutcome::NoOp("goal_gen_invalid_plan_update")
            }
            (_, RuntimeEvent::Debug(debug)) if debug.kind == "route_objective_contradiction" => {
                self.objective_trend_state.record_route_objective_contradiction();
                EventOutcome::NoOp("goal_gen_route_objective_contradiction")
            }
            (_, RuntimeEvent::Debug(debug)) if debug.kind == "goal_objective_drift" => {
                self.objective_trend_state.record_goal_objective_drift();
                EventOutcome::NoOp("goal_gen_goal_objective_drift")
            }
            (_, RuntimeEvent::Debug(_)) => EventOutcome::NoOp("goal_gen_debug_ignored"),
            (_, RuntimeEvent::ErrorOccurred(_)) => EventOutcome::NoOp("goal_gen_error_ignored"),
            (State::Waiting, RuntimeEvent::CapabilityCompleted(_)) | (State::Waiting, RuntimeEvent::CapabilityFailed(_)) => EventOutcome::NoOp("goal_gen_waiting_unrelated"),
            (State::Done, _) => EventOutcome::NoOp("goal_gen_noop"),
            (_, RuntimeEvent::Tick(_))
            | (_, RuntimeEvent::Code(_))
            | (_, RuntimeEvent::Edit(_))
            | (_, RuntimeEvent::LoopPlanned(_))
            | (_, RuntimeEvent::LoopActed(_))
            | (_, RuntimeEvent::LoopVerified(_))
            | (_, RuntimeEvent::LoopRewarded(_))
            | (_, RuntimeEvent::RouteTick(_))
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
            | (_, RuntimeEvent::InvariantDiscovered(_))
            | (_, RuntimeEvent::RustcCaptureStarted(_))
            | (_, RuntimeEvent::RustcGraphArtifactWritten(_))
            | (_, RuntimeEvent::RustcCaptureCompleted(_))
            | (_, RuntimeEvent::RustcCaptureFailed(_))
            | (_, RuntimeEvent::VerifierPolicyUpdated(_)) => EventOutcome::NoOp("goal_gen_noop"),
        }
    }
}

fn current_primary_objective(
    semantic_summary: &SemanticStateSummary,
    objective_trend_state: &ObjectiveTrendState,
) -> DevelopmentObjectiveKind {
    let objective_state = derive_self_development_objective_state(semantic_summary, 0, &[], objective_trend_state);
    primary_development_objective_kind(&objective_state, objective_trend_state, semantic_summary)
}

fn current_primary_strategy(
    semantic_summary: &SemanticStateSummary,
    objective_trend_state: &ObjectiveTrendState,
) -> DevelopmentStrategyKind {
    let objective_state = derive_self_development_objective_state(semantic_summary, 0, &[], objective_trend_state);
    primary_development_strategy_kind(&objective_state, objective_trend_state, semantic_summary)
}

fn infer_goal_objective(goal_text: &str) -> Option<DevelopmentObjectiveKind> {
    let lower = goal_text.to_ascii_lowercase();
    if lower.contains("test") || lower.contains("coverage") {
        Some(DevelopmentObjectiveKind::IncreaseTestCoverage)
    } else if lower.contains("cohesion") || lower.contains("module") || lower.contains("refactor") {
        Some(DevelopmentObjectiveKind::ImproveModuleCohesion)
    } else if lower.contains("invalid plan") || lower.contains("planning") {
        Some(DevelopmentObjectiveKind::DecreaseInvalidPlanRate)
    } else if lower.contains("contradiction") || lower.contains("drift") || lower.contains("align") {
        Some(DevelopmentObjectiveKind::ReduceContradictionRate)
    } else if lower.contains("stall") || lower.contains("progress") {
        Some(DevelopmentObjectiveKind::ReduceStalledLoopFrequency)
    } else if lower.contains("fix")
        || lower.contains("repair")
        || lower.contains("resolve")
        || lower.contains("compile")
        || lower.contains("error")
    {
        Some(DevelopmentObjectiveKind::ReduceCompilerFailures)
    } else {
        None
    }
}

fn goal_objective_drift(
    goal_objective: Option<DevelopmentObjectiveKind>,
    route_objective: DevelopmentObjectiveKind,
) -> bool {
    goal_objective.is_some_and(|goal| goal != route_objective)
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

fn goal_gen_prompt(
    objective: DevelopmentObjectiveKind,
    strategy: DevelopmentStrategyKind,
) -> anyhow::Result<String> {
    let registry = global_registry();
    let selected = registry.select_for_scope("goal_gen", objective, strategy)?;
    if !selected.is_empty() {
        return Ok(selected
            .into_iter()
            .map(|skill| skill.prompt.trim().to_string())
            .collect::<Vec<_>>()
            .join("\n\n"));
    }
    Ok(registry.load("goal_gen/generate_goal")?.prompt.clone())
}

fn validate_goal(content: &str) -> bool {
    if content.contains("goal-pending") {
        return false;
    }
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
        let event = RuntimeEvent::PromptLoaded(goal_prompt_loaded_event(content));
        em.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
    }
}
