use canon_event::{CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityRequested, EventConsumer, EventEmitterHandle, EventFilter, LoopActed, LoopObserved, LoopPlanned, PromptLoaded, Tick};
use serde_json::Value;
use uuid::Uuid;

const LLM_TIMEOUT_TICKS: u64 = 60;

pub struct PlanConsumer {
    emitter: Option<EventEmitterHandle>,
    pending: Option<PendingPlan>,
    last_observed: Option<LoopObserved>,
    last_planned_observed_tick: Option<u64>,
    /// Goal text that most recently produced a `done` response.
    /// While `observed.goal_text == last_done_goal` and there are no errors,
    /// the planner emits `no_op` instead of calling the LLM again.
    last_done_goal: Option<String>,
    /// Result of the most recent action — included in the next LLM prompt so
    /// the planner can react to failures (e.g. command not found, dir exists).
    last_acted: Option<LoopActed>,
    /// Last goal text that was fully inlined into the planner prompt.
    last_prompted_goal: Option<String>,
}

struct PendingPlan {
    tick: u64,
    request_id: String,
    dispatched_at_tick: u64,
    /// Goal text that was active when this LLM request was dispatched.
    goal_text: Option<String>,
    trace_id: String,
    execution_id: String,
    span_id: String,
    plan_id: String,
}

impl PlanConsumer {
    pub fn new() -> Self {
        Self {
            emitter: None,
            pending: None,
            last_observed: None,
            last_planned_observed_tick: None,
            last_done_goal: None,
            last_acted: None,
            last_prompted_goal: None,
        }
    }
}

impl EventConsumer for PlanConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::Tick(Tick { tick }) => {
                self.check_llm_timeout(*tick);
            }
            CanonEvent::LoopObserved(observed) => {
                self.last_observed = Some(observed.clone());
            }
            CanonEvent::Debug(debug) if debug.kind == "route_selected" => {
                let lane = debug
                    .payload
                    .get("approved_route")
                    .or_else(|| debug.payload.get("lane"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tick = debug
                    .payload
                    .get("tick")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                self.check_llm_timeout(tick);
                if lane == "shape" {
                    if let Some(observed) = self.last_observed.clone() {
                        self.handle_observed(&observed);
                    }
                }
            }
            CanonEvent::LoopActed(acted) => {
                self.last_acted = Some(acted.clone());
            }
            CanonEvent::CapabilityCompleted(payload) => {
                self.handle_capability_completed(payload);
            }
            CanonEvent::CapabilityFailed(payload) => {
                self.handle_capability_failed(payload);
            }
            CanonEvent::PromptLoaded(prompt) => {
                self.handle_prompt_loaded(prompt);
            }
            _ => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}

impl PlanConsumer {
    fn check_llm_timeout(&mut self, current_tick: u64) {
        let Some(pending) = &self.pending else { return; };
        if current_tick.saturating_sub(pending.dispatched_at_tick) < LLM_TIMEOUT_TICKS {
            return;
        }
        let tick = pending.tick;
        self.pending = None;
        self.emit_plan(LoopPlanned {
            tick,
            action_kind: "no_op".to_string(),
            action_payload: serde_json::json!({}),
            reason: "llm_timeout".to_string(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
        });
    }

    fn handle_prompt_loaded(&mut self, prompt: &PromptLoaded) {
        let is_goal = prompt
            .payload
            .get("prompt_id")
            .and_then(|v| v.as_str())
            .map(|id| id == "AGENT_GOAL")
            .unwrap_or(false)
            || prompt
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| p.contains("AGENT_GOAL"))
                .unwrap_or(false);
        if is_goal {
            // New goal arrived — clear the done-guard so the LLM gets called.
            self.last_done_goal = None;
            self.last_prompted_goal = None;
            // Also cancel any in-flight pending so we don't wait for the old LLM call.
            self.pending = None;
        }
    }

    fn handle_observed(&mut self, observed: &LoopObserved) {
        if self.pending.is_some() {
            return;
        }
        if self.last_planned_observed_tick == Some(observed.tick) {
            return;
        }
        if observed.goal_text.is_none() && observed.error_count == 0 {
            self.emit_plan(LoopPlanned {
                tick: observed.tick,
                action_kind: "no_op".to_string(),
                action_payload: serde_json::json!({}),
                reason: "no_goal".to_string(),
                llm_request_id: None,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
            });
            return;
        }
        // Goal already completed — don't call the LLM again unless errors appear.
        if observed.error_count == 0
            && self.last_done_goal.is_some()
            && self.last_done_goal == observed.goal_text
        {
            self.emit_plan(LoopPlanned {
                tick: observed.tick,
                action_kind: "no_op".to_string(),
                action_payload: serde_json::json!({}),
                reason: "goal_complete".to_string(),
                llm_request_id: None,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
            });
            return;
        }

        let request_id = Uuid::new_v4().to_string();
        let trace_id = Uuid::new_v4().to_string();
        let execution_id = Uuid::new_v4().to_string();
        let span_id = Uuid::new_v4().to_string();
        let plan_id = Uuid::new_v4().to_string();
        let include_full_goal = observed.goal_text != self.last_prompted_goal;
        let prompt = build_prompt(observed, self.last_acted.as_ref(), include_full_goal);
        let request = CapabilityRequested {
            request_id: request_id.clone(),
            name: "llm.call".to_string(),
            args: serde_json::json!({
                "prompt": prompt,
                "role": "planner",
            }),
        };

        self.pending = Some(PendingPlan {
            tick: observed.tick,
            request_id: request_id.clone(),
            dispatched_at_tick: observed.tick,
            goal_text: observed.goal_text.clone(),
            trace_id,
            execution_id,
            span_id,
            plan_id,
        });
        if include_full_goal {
            self.last_prompted_goal = observed.goal_text.clone();
        }
        self.last_planned_observed_tick = Some(observed.tick);

        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::CapabilityRequested(request));
        }
    }

    fn handle_capability_completed(&mut self, payload: &CapabilityCompleted) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.request_id != payload.request_id {
            self.pending = Some(pending);
            return;
        }

        let actions = parse_llm_actions(&payload.result);
        if actions.is_empty() {
            self.emit_plan(LoopPlanned {
                tick: pending.tick,
                action_kind: "no_op".to_string(),
                action_payload: serde_json::json!({}),
                reason: "llm_parse_failed".to_string(),
                llm_request_id: Some(pending.request_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: None,
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: None,
                action_id: None,
            });
            return;
        }

        let req_id = pending.request_id.clone();
        for action in actions {
            let plan_step_id = Uuid::new_v4().to_string();
            let action_id = plan_step_id.clone();
            let planned_span_id = Uuid::new_v4().to_string();
            match action {
                LlmAction::Patch { path, old, new } => {
                    self.emit_plan(LoopPlanned {
                        tick: pending.tick,
                        action_kind: "patch_file".to_string(),
                        action_payload: serde_json::json!({
                            "path": path,
                            "old": old,
                            "new": new,
                        }),
                        reason: "llm_patch".to_string(),
                        llm_request_id: Some(req_id.clone()),
                        trace_id: Some(pending.trace_id.clone()),
                        execution_id: Some(pending.execution_id.clone()),
                        span_id: Some(planned_span_id),
                        parent_span_id: Some(pending.span_id.clone()),
                        plan_id: Some(pending.plan_id.clone()),
                        plan_step_id: Some(plan_step_id),
                        action_id: Some(action_id),
                    });
                }
                LlmAction::Command { cmd, cwd } => {
                    self.emit_plan(LoopPlanned {
                        tick: pending.tick,
                        action_kind: "run_command".to_string(),
                        action_payload: action_payload_with_cwd(cmd, cwd),
                        reason: "llm_command".to_string(),
                        llm_request_id: Some(req_id.clone()),
                        trace_id: Some(pending.trace_id.clone()),
                        execution_id: Some(pending.execution_id.clone()),
                        span_id: Some(planned_span_id),
                        parent_span_id: Some(pending.span_id.clone()),
                        plan_id: Some(pending.plan_id.clone()),
                        plan_step_id: Some(plan_step_id),
                        action_id: Some(action_id),
                    });
                }
                LlmAction::Write { path, content } => {
                    self.emit_plan(LoopPlanned {
                        tick: pending.tick,
                        action_kind: "write_file".to_string(),
                        action_payload: serde_json::json!({
                            "path": path,
                            "content": content,
                        }),
                        reason: "llm_write".to_string(),
                        llm_request_id: Some(req_id.clone()),
                        trace_id: Some(pending.trace_id.clone()),
                        execution_id: Some(pending.execution_id.clone()),
                        span_id: Some(planned_span_id),
                        parent_span_id: Some(pending.span_id.clone()),
                        plan_id: Some(pending.plan_id.clone()),
                        plan_step_id: Some(plan_step_id),
                        action_id: Some(action_id),
                    });
                }
                LlmAction::Done { reason } => {
                    // Remember which goal was completed so we don't re-dispatch
                    // until a new goal arrives via PromptLoaded.
                    self.last_done_goal = pending.goal_text.clone();
                    self.emit_plan(LoopPlanned {
                        tick: pending.tick,
                        action_kind: "done".to_string(),
                        action_payload: serde_json::json!({}),
                        reason,
                        llm_request_id: Some(req_id.clone()),
                        trace_id: Some(pending.trace_id.clone()),
                        execution_id: Some(pending.execution_id.clone()),
                        span_id: Some(planned_span_id),
                        parent_span_id: Some(pending.span_id.clone()),
                        plan_id: Some(pending.plan_id.clone()),
                        plan_step_id: Some(plan_step_id),
                        action_id: Some(action_id),
                    });
                }
            }
        }
    }

    fn handle_capability_failed(&mut self, payload: &CapabilityFailed) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.request_id != payload.request_id {
            self.pending = Some(pending);
            return;
        }

        self.emit_plan(LoopPlanned {
            tick: pending.tick,
            action_kind: "no_op".to_string(),
            action_payload: serde_json::json!({}),
            reason: "llm_failed".to_string(),
            llm_request_id: Some(pending.request_id),
            trace_id: Some(pending.trace_id),
            execution_id: Some(pending.execution_id),
            span_id: None,
            parent_span_id: Some(pending.span_id),
            plan_id: Some(pending.plan_id),
            plan_step_id: None,
            action_id: None,
        });
    }

    fn emit_plan(&self, payload: LoopPlanned) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopPlanned(payload));
        }
    }
}

enum LlmAction {
    Patch { path: String, old: String, new: String },
    Write { path: String, content: String },
    Command { cmd: String, cwd: Option<String> },
    Done { reason: String },
}

/// Extract all ```json ... ``` blocks from a text string.
fn extract_fenced_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("```") {
        let after_fence = &remaining[start + 3..];
        // Skip optional language tag on the same line (e.g. "json")
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        let Some(end) = content.find("```") else { break; };
        blocks.push(content[..end].trim());
        remaining = &content[end + 3..];
    }
    blocks
}

fn parse_value_to_action(value: Value) -> Option<LlmAction> {
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("done")
            .to_string();
        return Some(LlmAction::Done { reason });
    }
    if let Some(cmd) = value.get("cmd").and_then(|v| v.as_str()) {
        let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        return Some(LlmAction::Command { cmd: cmd.to_string(), cwd });
    }
    if let (Some(write_path), Some(content)) = (
        value.get("write").and_then(|v| v.as_str()),
        value.get("content").and_then(|v| v.as_str()),
    ) {
        return Some(LlmAction::Write {
            path: write_path.to_string(),
            content: content.to_string(),
        });
    }
    let path = value.get("path").and_then(|v| v.as_str())?;
    let old = value.get("old").and_then(|v| v.as_str())?;
    let new = value.get("new").and_then(|v| v.as_str())?;
    Some(LlmAction::Patch {
        path: path.to_string(),
        old: old.to_string(),
        new: new.to_string(),
    })
}

/// Parse all actions from an LLM result. Supports multiple ```json blocks.
fn parse_llm_actions(result: &Value) -> Vec<LlmAction> {
    let mut value = result.clone();
    if let Some(inner) = value.get("result") {
        value = inner.clone();
    }

    // If the result has a "text" field, extract fenced blocks from it.
    if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
        let blocks = extract_fenced_blocks(text);
        if !blocks.is_empty() {
            return blocks
                .into_iter()
                .filter_map(|block| serde_json::from_str::<Value>(block).ok())
                .filter_map(parse_value_to_action)
                .collect();
        }
        // No fenced blocks found — try parsing the whole text as JSON
        if let Ok(parsed) = serde_json::from_str::<Value>(text.trim()) {
            return parse_value_to_action(parsed).into_iter().collect();
        }
        return vec![];
    }

    // Result is already a JSON object (no text wrapper)
    parse_value_to_action(value).into_iter().collect()
}

fn build_prompt(observed: &LoopObserved, last_acted: Option<&LoopActed>, include_full_goal: bool) -> String {
    let goal_section = match (observed.goal_text.as_ref(), include_full_goal) {
        (Some(text), true) => format!("## Active Goal\n{text}\n\n"),
        (Some(_), false) => "## Active Goal\n(unchanged from previous planner request)\n\n".to_string(),
        (None, _) => String::new(),
    };
    let error_section = if observed.error_count > 0 {
        let first = observed
            .compiler_errors
            .first()
            .and_then(|e| e.get("message"))
            .and_then(|m| m.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        format!(
            "## Compiler Errors ({})\nFirst error: {}\n\n",
            observed.error_count, first
        )
    } else {
        "## Compiler Errors\nNone.\n\n".to_string()
    };
    let last_action_section = match last_acted {
        None => String::new(),
        Some(a) => {
            let status = if a.success { "succeeded" } else { "FAILED" };
            let mut s = format!(
                "## Last Action Result\naction: {}\nstatus: {} (exit_code: {:?})\ncapability_request_id: {}\n",
                a.action_kind, status, a.exit_code, a.capability_request_id
            );
            if let Some(tool_call_id) = a.tool_call_id.as_ref() {
                s.push_str(&format!("tool_call_id: {}\n", tool_call_id));
            }
            if let Some(tool_result_id) = a.tool_result_id.as_ref() {
                s.push_str(&format!("tool_result_id: {}\n", tool_result_id));
            }
            if !a.stdout.is_empty() {
                s.push_str(&format!("stdout: {}\n", a.stdout.trim()));
            }
            if !a.stderr.is_empty() {
                s.push_str(&format!("stderr: {}\n", a.stderr.trim()));
            }
            s.push('\n');
            s
        }
    };
    format!(
        "{goal}{last_action}{errors}Return one or more fenced ```json code blocks (no prose outside code blocks). Each block must be one action object using one schema:\n- Run a command:  {{\"cmd\": \"cargo new foo\", \"cwd\": \"/path\"}}\n- Write a file:   {{\"write\": \"/abs/path\", \"content\": \"full content\"}}\n- Patch a file:   {{\"path\": \"/abs/path\", \"old\": \"exact text\", \"new\": \"replacement\"}}\n- Signal done:    {{\"done\": true, \"reason\": \"...\"}}",
        goal = goal_section,
        last_action = last_action_section,
        errors = error_section,
    )
}

fn action_payload_with_cwd(cmd: String, cwd: Option<String>) -> Value {
    if let Some(cwd) = cwd {
        serde_json::json!({ "cmd": cmd, "cwd": cwd })
    } else {
        serde_json::json!({ "cmd": cmd })
    }
}
