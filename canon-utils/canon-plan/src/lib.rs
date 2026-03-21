use canon_event::{
    CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, LlmCall, LoopActed, LoopObserved, LoopPlanned, PromptLoaded, Tick, ToolCall,
    ToolResult,
};
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
    /// Results from the last completed action batch, fed to the next planner call.
    batch_acted: Vec<LoopActed>,
    batch_tool_results: Vec<ToolResult>,
    /// Last goal text that was fully inlined into the planner prompt.
    last_prompted_goal: Option<String>,
    workspace: std::path::PathBuf,
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
    /// Synthetic tool_call_id emitted as CanonEvent::ToolCall to block the routing gate.
    plan_tool_call_id: String,
}

impl PlanConsumer {
    pub fn new(workspace: std::path::PathBuf) -> Self {
        Self {
            emitter: None,
            pending: None,
            last_observed: None,
            last_planned_observed_tick: None,
            last_done_goal: None,
            batch_acted: Vec::new(),
            batch_tool_results: Vec::new(),
            last_prompted_goal: None,
            workspace,
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
                let lane = debug.payload.get("approved_route").or_else(|| debug.payload.get("lane")).and_then(|v| v.as_str()).unwrap_or("");
                let tick = debug.payload.get("tick").and_then(|v| v.as_u64()).unwrap_or(0);
                self.check_llm_timeout(tick);
                if lane == "shape" {
                    if let Some(observed) = self.last_observed.clone() {
                        self.handle_observed(&observed);
                    }
                }
            }
            CanonEvent::LoopActed(acted) => {
                self.batch_acted.push(acted.clone());
                if !acted.success {
                    self.last_planned_observed_tick = None;
                }
            }
            CanonEvent::ToolResult(result) if result.kind != "llm.plan" => {
                self.batch_tool_results.push(result.clone());
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
    fn requirements_satisfied(&self, observed: &LoopObserved) -> bool {
        let Some(goal_text) = observed.goal_text.as_ref() else {
            return false;
        };
        let required_loc = goal_text
            .lines()
            .filter(|l| l.to_lowercase().contains("loc"))
            .find_map(|l| {
                let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
                digits.parse::<usize>().ok()
            })
            .unwrap_or(0);
        if required_loc == 0 {
            return true;
        }
        let actual_loc = count_loc_in_workspace(&self.workspace);
        actual_loc >= required_loc
    }

    fn check_llm_timeout(&mut self, current_tick: u64) {
        let Some(pending) = &self.pending else {
            return;
        };
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
        let is_goal = prompt.payload.get("prompt_id").and_then(|v| v.as_str()).map(|id| id == "AGENT_GOAL").unwrap_or(false)
            || prompt.payload.get("path").and_then(|v| v.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false);
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
        // Only suppress LLM call if done was previously declared and requirements
        // are actually satisfied in the workspace.
        if observed.error_count == 0 && self.last_done_goal.is_some() && self.last_done_goal == observed.goal_text {
            if self.requirements_satisfied(observed) {
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
            self.last_done_goal = None;
        }

        let request_id = Uuid::new_v4().to_string();
        let trace_id = Uuid::new_v4().to_string();
        let execution_id = Uuid::new_v4().to_string();
        let span_id = Uuid::new_v4().to_string();
        let plan_id = Uuid::new_v4().to_string();
        let include_full_goal = observed.goal_text != self.last_prompted_goal;
        let prompt = build_prompt(observed, &self.batch_acted, &self.batch_tool_results, include_full_goal, &self.workspace);
        let _last_action_results_payload: Vec<Value> = self
            .batch_acted
            .iter()
            .map(|a| {
                serde_json::json!({
                    "action_kind": a.action_kind,
                    "success": a.success,
                    "exit_code": a.exit_code,
                    "capability_request_id": a.capability_request_id,
                    "tool_call_id": a.tool_call_id,
                    "tool_result_id": a.tool_result_id,
                    "stdout": a.stdout,
                    "stderr": a.stderr,
                })
            })
            .collect();
        let _last_tool_results_payload: Vec<Value> = self
            .batch_tool_results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "node_id": r.node_id,
                    "tool_call_id": r.tool_call_id,
                    "tool_result_id": r.tool_result_id,
                    "request_id": r.request_id,
                    "kind": r.kind,
                    "output": r.output,
                    "success": r.success,
                })
            })
            .collect();
        let llm_call = LlmCall { request_id: request_id.clone(), prompt, role: Some("planner".to_string()) };

        let plan_tool_call_id = Uuid::new_v4().to_string();
        self.batch_acted.clear();
        self.batch_tool_results.clear();
        self.pending = Some(PendingPlan {
            tick: observed.tick,
            request_id: request_id.clone(),
            dispatched_at_tick: observed.tick,
            goal_text: observed.goal_text.clone(),
            trace_id,
            execution_id,
            span_id,
            plan_id,
            plan_tool_call_id: plan_tool_call_id.clone(),
        });
        if include_full_goal {
            self.last_prompted_goal = observed.goal_text.clone();
        }
        self.last_planned_observed_tick = Some(observed.tick);

        if let Some(emitter) = self.emitter.as_ref() {
            canon_meta::canon_emit_meta!(emitter; ToolCall(ToolCall {
                node_id: "plan_consumer".to_string(),
                tool_call_id: plan_tool_call_id,
                request_id: request_id.clone(),
                kind: "llm.plan".to_string(),
                payload: serde_json::json!({"role": "planner"}),
            }));
            canon_meta::canon_emit_meta!(emitter; Llm(llm_call));
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

        // Unblock the routing gate — matches the ToolCall emitted in handle_observed.
        self.emit_tool_result(&pending.plan_tool_call_id, &pending.request_id, true);

        let actions = match &payload.result {
            CapabilityResult::Llm(llm) => parse_llm_actions(&llm.response),
            _ => Vec::new(),
        };
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
                    if let Some(goal_text) = &pending.goal_text {
                        let required_loc = goal_text
                            .lines()
                            .filter(|l| l.to_lowercase().contains("loc"))
                            .find_map(|l| {
                                let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
                                digits.parse::<usize>().ok()
                            })
                            .unwrap_or(0);
                        let satisfied = required_loc == 0 || count_loc_in_workspace(&self.workspace) >= required_loc;
                        if satisfied {
                            self.last_done_goal = pending.goal_text.clone();
                        }
                    } else {
                        self.last_done_goal = pending.goal_text.clone();
                    }
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

        // Unblock the routing gate — matches the ToolCall emitted in handle_observed.
        self.emit_tool_result(&pending.plan_tool_call_id, &pending.request_id, false);

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
            canon_meta::canon_emit_meta!(emitter; LoopPlanned(payload));
        }
    }

    fn emit_tool_result(&self, tool_call_id: &str, request_id: &str, success: bool) {
        if let Some(emitter) = self.emitter.as_ref() {
            canon_meta::canon_emit_meta!(emitter; ToolResult(ToolResult {
                node_id: "plan_consumer".to_string(),
                tool_call_id: tool_call_id.to_string(),
                tool_result_id: Uuid::new_v4().to_string(),
                request_id: request_id.to_string(),
                kind: "llm.plan".to_string(),
                output: serde_json::json!({}),
                success,
            }));
        }
    }
}

enum LlmAction {
    Patch { path: String, old: String, new: String },
    Write { path: String, content: String },
    Command { cmd: String, cwd: Option<String> },
    Done { reason: String },
}

/// Extract fenced code blocks by line-delimited fences only.
/// This avoids false closes from embedded ``` inside JSON string literals.
fn extract_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") {
                in_fence = true;
                current.clear();
            }
            continue;
        }

        if trimmed.starts_with("```") {
            blocks.push(current.trim().to_string());
            in_fence = false;
            current.clear();
            continue;
        }

        current.push_str(line);
        current.push('\n');
    }

    blocks
}

fn parse_value_to_action(value: Value) -> Option<LlmAction> {
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        let reason = value.get("reason").and_then(|v| v.as_str()).unwrap_or("done").to_string();
        return Some(LlmAction::Done { reason });
    }
    if let Some(cmd) = value.get("cmd").and_then(|v| v.as_str()) {
        let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        return Some(LlmAction::Command { cmd: cmd.to_string(), cwd });
    }
    if let (Some(write_path), Some(content)) = (value.get("write").and_then(|v| v.as_str()), value.get("content").and_then(|v| v.as_str())) {
        return Some(LlmAction::Write { path: write_path.to_string(), content: content.to_string() });
    }
    let path = value.get("path").and_then(|v| v.as_str())?;
    let old = value.get("old").and_then(|v| v.as_str())?;
    let new = value.get("new").and_then(|v| v.as_str())?;
    Some(LlmAction::Patch { path: path.to_string(), old: old.to_string(), new: new.to_string() })
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
            return blocks.iter().filter_map(|block| serde_json::from_str::<Value>(block).ok()).filter_map(parse_value_to_action).collect();
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

fn build_prompt(observed: &LoopObserved, batch_acted: &[LoopActed], batch_tool_results: &[ToolResult], include_full_goal: bool, workspace: &std::path::Path) -> String {
    let workspace_state_section = build_workspace_state_section(observed, workspace);
    let goal_section = match (observed.goal_text.as_ref(), include_full_goal) {
        (Some(text), true) => format!("## Active Goal\n{text}\n\n"),
        (Some(_), false) => "## Active Goal\n(unchanged from previous planner request)\n\n".to_string(),
        (None, _) => String::new(),
    };
    let progress_section = build_progress_section(observed, workspace);
    let error_section = if observed.error_count > 0 {
        let first = observed.compiler_errors.first().and_then(|e| e.get("message")).and_then(|m| m.get("message")).and_then(|v| v.as_str()).unwrap_or("unknown error");
        format!("## Compiler Errors ({})\nFirst error: {}\n\n", observed.error_count, first)
    } else {
        "## Compiler Errors\nNone.\n\n".to_string()
    };
    let last_action_section = if batch_acted.is_empty() {
        String::new()
    } else {
        let mut s = format!("## Action Results ({} actions)\n", batch_acted.len());
        for (i, a) in batch_acted.iter().enumerate() {
            let status = if a.success { "succeeded" } else { "FAILED" };
            s.push_str(&format!("### Action {}\naction: {}\nstatus: {} (exit_code: {:?})\ncapability_request_id: {}\n", i + 1, a.action_kind, status, a.exit_code, a.capability_request_id));
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
        }
        s
    };
    let last_tool_result_section = if batch_tool_results.is_empty() {
        String::new()
    } else {
        let mut s = format!("## Tool Results ({} results)\n", batch_tool_results.len());
        for (i, result) in batch_tool_results.iter().enumerate() {
            let mut output = serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| result.output.to_string());
            if output.len() > 2000 {
                output.truncate(2000);
                output.push_str("\n...<truncated>");
            }
            s.push_str(&format!(
                "### Result {}\ntool_kind: {}\nrequest_id: {}\ntool_call_id: {}\ntool_result_id: {}\nsuccess: {}\noutput:\n{}\n\n",
                i + 1,
                result.kind,
                result.request_id,
                result.tool_call_id,
                result.tool_result_id,
                result.success,
                output,
            ));
        }
        s
    };
    format!(
        "{workspace}{goal}{progress}{last_action}{last_tool_result}{errors}Execution policy constraints:\n- Do NOT emit destructive commands (`rm -rf`, `git reset --hard`, `git clean -f`, `dd`, `mkfs`, `shred`).\n- If a target directory already exists, prefer `cargo init --bin <dir>` instead of deleting and recreating it.\n\nGeneration policy:\n- You MUST generate LARGE amounts of code per shape step. Each `write` action should contain hundreds to thousands of lines of Rust.\n- A single shape step should advance LOC by thousands, not tens. Write entire modules at once.\n- Do not declare done until the LOC progress section shows >= required LOC.\n\nReturn one or more fenced ```json code blocks (no prose outside code blocks). Each block must be one action object using one schema:\n- Run a command:  {{\"cmd\": \"cargo new foo\", \"cwd\": \"/path\"}}\n- Write a file:   {{\"write\": \"/abs/path\", \"content\": \"full content\"}}\n- Patch a file:   {{\"path\": \"/abs/path\", \"old\": \"exact text\", \"new\": \"replacement\"}}\n- Signal done:    {{\"done\": true, \"reason\": \"...\"}}",
        workspace = workspace_state_section,
        goal = goal_section,
        progress = progress_section,
        last_action = last_action_section,
        last_tool_result = last_tool_result_section,
        errors = error_section,
    )
}

fn build_workspace_state_section(observed: &LoopObserved, workspace: &std::path::Path) -> String {
    let target = resolve_target_project_dir(observed, workspace);
    if target.is_dir() {
        let cargo_toml = target.join("Cargo.toml").exists();
        let src_main = target.join("src/main.rs").exists();
        let entries = std::fs::read_dir(&target).ok().into_iter().flat_map(|it| it.flatten()).take(20).filter_map(|entry| entry.file_name().into_string().ok()).collect::<Vec<_>>();
        let contents = if entries.is_empty() { "(empty)".to_string() } else { entries.join(", ") };
        format!(
            "## Workspace State\nTarget directory: {} - EXISTS\nCargo.toml present: {}\nsrc/main.rs present: {}\nContents: {}\nDirective: Use `cargo init` or write files directly. Do NOT run `cargo new` for this path.\n\n",
            target.display(),
            cargo_toml,
            src_main,
            contents
        )
    } else {
        format!("## Workspace State\nTarget directory: {} - DOES NOT EXIST\nDirective: Use `cargo new` or `cargo init` to create the project directory.\n\n", target.display())
    }
}

fn resolve_target_project_dir(observed: &LoopObserved, workspace: &std::path::Path) -> std::path::PathBuf {
    observed
        .goal_text
        .as_ref()
        .and_then(|goal_text| goal_text.lines().find_map(|l| l.trim().strip_prefix("- Project path:").map(|p| std::path::PathBuf::from(p.trim().trim_matches('`')))))
        .unwrap_or_else(|| workspace.join("test_rust_project_v3"))
}

fn build_progress_section(observed: &LoopObserved, workspace: &std::path::Path) -> String {
    let Some(goal_text) = observed.goal_text.as_ref() else {
        return String::new();
    };

    let required_loc: usize = goal_text
        .lines()
        .filter(|l| l.to_lowercase().contains("loc"))
        .find_map(|l| {
            let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        })
        .unwrap_or(0);
    if required_loc == 0 {
        return String::new();
    }

    let target = resolve_target_project_dir(observed, workspace);

    let actual_loc = count_loc_in_workspace(&target);
    let remaining = required_loc.saturating_sub(actual_loc);
    let pct = if required_loc > 0 { (actual_loc * 100) / required_loc } else { 100 };

    format!("## LOC Progress\nCurrent: {} lines / {} required ({}%)\nRemaining: {} lines to write\n\n", actual_loc, required_loc, pct, remaining)
}

fn action_payload_with_cwd(cmd: String, cwd: Option<String>) -> Value {
    if let Some(cwd) = cwd {
        serde_json::json!({ "cmd": cmd, "cwd": cwd })
    } else {
        serde_json::json!({ "cmd": cmd })
    }
}

fn count_loc_in_workspace(workspace: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(workspace) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc_in_workspace(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}
