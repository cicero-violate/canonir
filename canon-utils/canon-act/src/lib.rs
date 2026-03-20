use canon_event::{CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityRequested, EventConsumer, EventEmitterHandle, EventFilter, LoopActed, LoopPlanned};
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
    request_id: String,
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
            CanonEvent::LoopPlanned(planned) => self.handle_plan(planned),
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
    fn handle_plan(&mut self, planned: &LoopPlanned) {
        if self.pending.is_some() {
            self.queue.push_back(planned.clone());
            return;
        }
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };

        match planned.action_kind.as_str() {
            "no_op" | "done" => {
                emitter.emit(CanonEvent::LoopActed(LoopActed {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    capability_request_id: String::new(),
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
                // No capability pending — drain the next queued plan immediately
                if let Some(next) = self.queue.pop_front() {
                    let next_planned = next;
                    self.handle_plan(&next_planned);
                }
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
                    request_id,
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
                    request_id,
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
                    request_id,
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
        self.emit_acted(pending, stdout, stderr, exit_code, duration_ms, success);
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
        self.emit_acted(pending, String::new(), payload.error.clone(), None, duration_ms, false);
    }

    fn check_timeout(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.started_at.elapsed() <= Duration::from_secs(30) {
            self.pending = Some(pending);
            return;
        }
        self.emit_acted(pending, String::new(), "timeout".to_string(), None, 30_000, false);
    }

    fn emit_acted(
        &mut self,
        pending: PendingAct,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
        duration_ms: u64,
        success: bool,
    ) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopActed(LoopActed {
                tick: pending.tick,
                action_kind: pending.action_kind,
                capability_request_id: pending.request_id,
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
        // Start the next queued plan immediately
        if let Some(next) = self.queue.pop_front() {
            self.handle_plan(&next);
        }
    }

    fn emit_missing_args(&self, planned: &LoopPlanned, reason: &str) {
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopActed(LoopActed {
                tick: planned.tick,
                action_kind: planned.action_kind.clone(),
                capability_request_id: String::new(),
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
