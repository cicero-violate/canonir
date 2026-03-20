use canon_event::{CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityRequested, EventConsumer, EventEmitterHandle, EventFilter, LoopActed, LoopPlanned, ToolCall, ToolResult};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub struct ActConsumer {
    emitter: Option<EventEmitterHandle>,
    pending: Option<PendingAct>,
    queue: VecDeque<LoopPlanned>,
}

struct PendingAct {
    tick: u64,
    action_kind: String,
    tool_kind: String,
    request_id: String,
    tool_call_id: String,
    node_id: String,
    started_at: Instant,
    trace_id: Option<String>,
    execution_id: Option<String>,
    parent_span_id: Option<String>,
    plan_id: Option<String>,
    plan_step_id: Option<String>,
    action_id: Option<String>,
}

impl ActConsumer {
    pub fn new() -> Self {
        Self {
            emitter: None,
            pending: None,
            queue: VecDeque::new(),
        }
    }
}

impl EventConsumer for ActConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::LoopPlanned(planned) => self.enqueue_plan(planned),
            CanonEvent::Debug(debug) if debug.kind == "route_selected" => {
                self.handle_route_selected(debug.payload.as_object());
            }
            CanonEvent::CapabilityCompleted(payload) => self.handle_completed(payload),
            CanonEvent::CapabilityFailed(payload) => self.handle_failed(payload),
            CanonEvent::Tick(_) => self.check_timeout(),
            _ => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}

impl ActConsumer {
    fn enqueue_plan(&mut self, planned: &LoopPlanned) {
        self.queue.push_back(planned.clone());
    }

    fn handle_route_selected(&mut self, payload: Option<&serde_json::Map<String, Value>>) {
        let Some(payload) = payload else {
            return;
        };
        let lane = payload
            .get("approved_route")
            .or_else(|| payload.get("lane"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if lane != "execute" {
            return;
        }
        if self.pending.is_some() {
            return;
        }
        if let Some(next) = self.queue.pop_front() {
            self.dispatch_plan(&next);
        }
    }

    fn dispatch_plan(&mut self, planned: &LoopPlanned) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };

        match planned.action_kind.as_str() {
            "no_op" | "done" => {
                emitter.emit(CanonEvent::LoopActed(LoopActed {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    capability_request_id: String::new(),
                    tool_call_id: None,
                    tool_result_id: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: None,
                    duration_ms: 0,
                    success: true,
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    span_id: Some(Uuid::new_v4().to_string()),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                }));
            }
            "run_command" => {
                let cmd = planned.action_payload.get("cmd").and_then(|v| v.as_str());
                let cwd = planned
                    .action_payload
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let Some(cmd) = cmd else {
                    self.emit_missing_args(planned, "missing_cmd");
                    return;
                };
                let request_id = Uuid::new_v4().to_string();
                let tool_call_id = Uuid::new_v4().to_string();
                let node_id = tool_node_id(planned);
                emitter.emit(CanonEvent::ToolCall(ToolCall {
                    node_id: node_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request_id: request_id.clone(),
                    kind: "bash".to_string(),
                    payload: serde_json::json!({
                        "cmd": cmd,
                        "cwd": cwd,
                    }),
                }));
                emitter.emit(CanonEvent::CapabilityRequested(CapabilityRequested {
                    request_id: request_id.clone(),
                    name: "bash".to_string(),
                    args: serde_json::json!({
                        "cmd": cmd,
                        "cwd": cwd,
                    }),
                }));
                self.pending = Some(PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "bash".to_string(),
                    request_id,
                    tool_call_id,
                    node_id,
                    started_at: Instant::now(),
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                });
            }
            "write_file" => {
                let path = planned.action_payload.get("path").and_then(|v| v.as_str());
                let content = planned.action_payload.get("content").and_then(|v| v.as_str());
                let (Some(path), Some(content)) = (path, content) else {
                    self.emit_missing_args(planned, "missing_path_or_content");
                    return;
                };
                let request_id = Uuid::new_v4().to_string();
                let tool_call_id = Uuid::new_v4().to_string();
                let node_id = tool_node_id(planned);
                emitter.emit(CanonEvent::ToolCall(ToolCall {
                    node_id: node_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request_id: request_id.clone(),
                    kind: "file.write".to_string(),
                    payload: serde_json::json!({
                        "path": path,
                        "content": content,
                    }),
                }));
                emitter.emit(CanonEvent::CapabilityRequested(CapabilityRequested {
                    request_id: request_id.clone(),
                    name: "file.write".to_string(),
                    args: serde_json::json!({
                        "path": path,
                        "content": content,
                    }),
                }));
                self.pending = Some(PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "file.write".to_string(),
                    request_id,
                    tool_call_id,
                    node_id,
                    started_at: Instant::now(),
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                });
            }
            "patch_file" => {
                let path = planned.action_payload.get("path").and_then(|v| v.as_str());
                let old = planned.action_payload.get("old").and_then(|v| v.as_str());
                let new = planned.action_payload.get("new").and_then(|v| v.as_str());
                let (Some(path), Some(old), Some(new)) = (path, old, new) else {
                    self.emit_missing_args(planned, "missing_patch_args");
                    return;
                };
                let request_id = Uuid::new_v4().to_string();
                let tool_call_id = Uuid::new_v4().to_string();
                let node_id = tool_node_id(planned);
                emitter.emit(CanonEvent::ToolCall(ToolCall {
                    node_id: node_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request_id: request_id.clone(),
                    kind: "file.patch".to_string(),
                    payload: serde_json::json!({
                        "path": path,
                        "old": old,
                        "new": new,
                    }),
                }));
                emitter.emit(CanonEvent::CapabilityRequested(CapabilityRequested {
                    request_id: request_id.clone(),
                    name: "file.patch".to_string(),
                    args: serde_json::json!({
                        "path": path,
                        "old": old,
                        "new": new,
                    }),
                }));
                self.pending = Some(PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "file.patch".to_string(),
                    request_id,
                    tool_call_id,
                    node_id,
                    started_at: Instant::now(),
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                });
            }
            _ => {
                self.emit_missing_args(planned, "unknown_action_kind");
            }
        }
    }

    fn handle_completed(&mut self, payload: &CapabilityCompleted) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.request_id != payload.request_id {
            self.pending = Some(pending);
            return;
        }
        let (stdout, stderr, exit_code, duration_ms, success) = extract_result_fields(&payload.result, pending.started_at);
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(
            &pending,
            tool_result_id.clone(),
            payload.result.clone(),
            success,
        );
        self.emit_acted(pending, stdout, stderr, exit_code, duration_ms, success, Some(tool_result_id));
    }

    fn handle_failed(&mut self, payload: &CapabilityFailed) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.request_id != payload.request_id {
            self.pending = Some(pending);
            return;
        }
        let duration_ms = pending.started_at.elapsed().as_millis() as u64;
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(
            &pending,
            tool_result_id.clone(),
            serde_json::json!({ "error": payload.error }),
            false,
        );
        self.emit_acted(
            pending,
            String::new(),
            payload.error.clone(),
            None,
            duration_ms,
            false,
            Some(tool_result_id),
        );
    }

    fn check_timeout(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.started_at.elapsed() <= Duration::from_secs(30) {
            self.pending = Some(pending);
            return;
        }
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(
            &pending,
            tool_result_id.clone(),
            serde_json::json!({ "error": "timeout" }),
            false,
        );
        self.emit_acted(
            pending,
            String::new(),
            "timeout".to_string(),
            None,
            30_000,
            false,
            Some(tool_result_id),
        );
    }

    fn emit_acted(
        &mut self,
        pending: PendingAct,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
        duration_ms: u64,
        success: bool,
        tool_result_id: Option<String>,
    ) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopActed(LoopActed {
                tick: pending.tick,
                action_kind: pending.action_kind,
                capability_request_id: pending.request_id,
                tool_call_id: Some(pending.tool_call_id),
                tool_result_id,
                stdout,
                stderr,
                exit_code,
                duration_ms,
                success,
                trace_id: pending.trace_id,
                execution_id: pending.execution_id,
                span_id: Some(Uuid::new_v4().to_string()),
                parent_span_id: pending.parent_span_id,
                plan_id: pending.plan_id,
                plan_step_id: pending.plan_step_id,
                action_id: pending.action_id,
            }));
        }
    }

    fn emit_missing_args(&self, planned: &LoopPlanned, reason: &str) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopActed(LoopActed {
                tick: planned.tick,
                action_kind: planned.action_kind.clone(),
                capability_request_id: String::new(),
                tool_call_id: None,
                tool_result_id: None,
                stdout: String::new(),
                stderr: reason.to_string(),
                exit_code: None,
                duration_ms: 0,
                success: false,
                trace_id: planned.trace_id.clone(),
                execution_id: planned.execution_id.clone(),
                span_id: Some(Uuid::new_v4().to_string()),
                parent_span_id: planned.span_id.clone(),
                plan_id: planned.plan_id.clone(),
                plan_step_id: planned.plan_step_id.clone(),
                action_id: planned.action_id.clone(),
            }));
        }
    }

    fn emit_tool_result(
        &self,
        pending: &PendingAct,
        tool_result_id: String,
        output: Value,
        success: bool,
    ) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::ToolResult(ToolResult {
                node_id: pending.node_id.clone(),
                tool_call_id: pending.tool_call_id.clone(),
                tool_result_id,
                request_id: pending.request_id.clone(),
                kind: pending.tool_kind.clone(),
                output,
                success,
            }));
        }
    }
}

fn tool_node_id(planned: &LoopPlanned) -> String {
    if let Some(action_id) = planned.action_id.as_ref() {
        return action_id.clone();
    }
    if let Some(plan_step_id) = planned.plan_step_id.as_ref() {
        return plan_step_id.clone();
    }
    format!("tick:{}:{}", planned.tick, planned.action_kind)
}

fn extract_result_fields(result: &Value, started_at: Instant) -> (String, String, Option<i32>, u64, bool) {
    let stdout = result.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let stderr = result.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let exit_code = result
        .get("status")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .or_else(|| result.get("exit_code").and_then(|v| v.as_i64()).map(|v| v as i32));
    let duration_ms = result
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| started_at.elapsed().as_millis() as u64);
    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
    (stdout, stderr, exit_code, duration_ms, success)
}
