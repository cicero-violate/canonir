use canon_event::{CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityRequested, DebugEvent, EventConsumer, EventEmitterHandle, EventFilter, LoopActed, LoopPlanned, ToolCall, ToolResult};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub struct ActConsumer {
    emitter: Option<EventEmitterHandle>,
    pending: Option<PendingAct>,
    queue: VecDeque<LoopPlanned>,
    workspace: PathBuf,
    artifact_dir: PathBuf,
    artifact_counter: u32,
    active_batch_llm_request_id: Option<String>,
    queued_artifact_index: HashMap<String, u32>,
    batch_tracker: HashMap<String, BatchStatus>,
    last_reconcile: Option<Instant>,
    destructive_cmd_policy: DestructiveCmdPolicy,
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
    artifact_n: u32,
    llm_request_id: Option<String>,
}

#[derive(Clone, Default)]
struct BatchStatus {
    artifact_n: u32,
    planned: usize,
    dispatched: usize,
    completed_ok: usize,
    completed_fail: usize,
}

#[derive(Clone, Copy)]
enum DestructiveCmdPolicy {
    Allow,
    Warn,
    Block,
}

impl DestructiveCmdPolicy {
    fn from_env() -> Self {
        match env::var("CANON_DESTRUCTIVE_CMD_POLICY").unwrap_or_else(|_| "block".to_string()).to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "warn" => Self::Warn,
            _ => Self::Block,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }
}

impl ActConsumer {
    pub fn new(workspace: PathBuf) -> Self {
        let artifact_dir = default_tool_artifact_dir();
        let artifact_counter = next_tool_artifact_counter(&artifact_dir);
        Self {
            emitter: None,
            pending: None,
            queue: VecDeque::new(),
            workspace,
            artifact_dir,
            artifact_counter,
            active_batch_llm_request_id: None,
            queued_artifact_index: HashMap::new(),
            batch_tracker: HashMap::new(),
            last_reconcile: None,
            destructive_cmd_policy: DestructiveCmdPolicy::from_env(),
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
            CanonEvent::Tick(_) => {
                self.check_timeout();
                self.reconcile_stale_pending_artifacts();
            }
            _ => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
        self.reconcile_stale_pending_artifacts();
    }
}

impl ActConsumer {
    fn enqueue_plan(&mut self, planned: &LoopPlanned) {
        self.queue.push_back(planned.clone());
        let artifact_n = self.artifact_index_for_plan(planned);
        self.write_tool_call_queued_artifact(artifact_n, planned);
        self.mark_batch_planned(planned, artifact_n);
    }

    fn handle_route_selected(&mut self, payload: Option<&serde_json::Map<String, Value>>) {
        let Some(payload) = payload else {
            return;
        };
        let lane = payload.get("approved_route").or_else(|| payload.get("lane")).and_then(|v| v.as_str()).unwrap_or("");
        if lane != "execute" {
            return;
        }
        self.dispatch_batch_on_execute();
    }

    fn dispatch_plan(&mut self, planned: &LoopPlanned) {
        let Some(emitter) = self.emitter.clone() else {
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
                self.mark_batch_inline_completion(planned, true);
            }
            "run_command" => {
                let cmd = planned.action_payload.get("cmd").and_then(|v| v.as_str());
                let cwd = planned.action_payload.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");
                let Some(cmd) = cmd else {
                    self.emit_missing_args(planned, "missing_cmd");
                    return;
                };
                if is_potentially_destructive(cmd, &self.workspace) {
                    match self.destructive_cmd_policy {
                        DestructiveCmdPolicy::Block => {
                            self.emit_missing_args(planned, "rejected_destructive_command");
                            return;
                        }
                        DestructiveCmdPolicy::Warn => {
                            if let Some(emitter) = self.emitter.as_ref() {
                                emitter.emit(CanonEvent::Debug(DebugEvent {
                                    source: "act_consumer".to_string(),
                                    kind: "destructive_command_warning".to_string(),
                                    payload: serde_json::json!({
                                        "cmd": cmd,
                                        "policy": self.destructive_cmd_policy.as_str(),
                                        "action_id": planned.action_id,
                                    }),
                                }));
                            }
                        }
                        DestructiveCmdPolicy::Allow => {}
                    }
                }
                let request_id = Uuid::new_v4().to_string();
                let tool_call_id = Uuid::new_v4().to_string();
                let node_id = tool_node_id(planned);
                let artifact_n = self.artifact_index_for_plan(planned);
                self.clear_cached_artifact_index_for_plan(planned);
                self.mark_batch_dispatched(planned);

                self.write_tool_call_artifact(
                    artifact_n,
                    "bash",
                    &node_id,
                    &tool_call_id,
                    &request_id,
                    &serde_json::json!({
                        "cmd": cmd,
                        "cwd": cwd,
                    }),
                );
                self.write_tool_result_pending_artifact(artifact_n, planned, "bash", &node_id, &tool_call_id, &request_id);

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
                    artifact_n,
                    llm_request_id: planned.llm_request_id.clone(),
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
                let artifact_n = self.artifact_index_for_plan(planned);
                self.clear_cached_artifact_index_for_plan(planned);
                self.mark_batch_dispatched(planned);

                self.write_tool_call_artifact(
                    artifact_n,
                    "file.write",
                    &node_id,
                    &tool_call_id,
                    &request_id,
                    &serde_json::json!({
                        "path": path,
                        "content": content,
                    }),
                );
                self.write_tool_result_pending_artifact(artifact_n, planned, "file.write", &node_id, &tool_call_id, &request_id);

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
                    artifact_n,
                    llm_request_id: planned.llm_request_id.clone(),
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
                let artifact_n = self.artifact_index_for_plan(planned);
                self.clear_cached_artifact_index_for_plan(planned);
                self.mark_batch_dispatched(planned);

                self.write_tool_call_artifact(
                    artifact_n,
                    "file.patch",
                    &node_id,
                    &tool_call_id,
                    &request_id,
                    &serde_json::json!({
                        "path": path,
                        "old": old,
                        "new": new,
                    }),
                );
                self.write_tool_result_pending_artifact(artifact_n, planned, "file.patch", &node_id, &tool_call_id, &request_id);

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
                    artifact_n,
                    llm_request_id: planned.llm_request_id.clone(),
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
        let llm_request_id = pending.llm_request_id.clone();
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(&pending, tool_result_id.clone(), payload.result.clone(), success);
        self.mark_batch_completion(llm_request_id.as_deref(), success);
        self.emit_acted(pending, stdout, stderr, exit_code, duration_ms, success, Some(tool_result_id));
        if !success {
            self.abort_active_batch();
        }
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
        let llm_request_id = pending.llm_request_id.clone();
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(&pending, tool_result_id.clone(), serde_json::json!({ "error": payload.error }), false);
        self.mark_batch_completion(llm_request_id.as_deref(), false);
        self.emit_acted(pending, String::new(), payload.error.clone(), None, duration_ms, false, Some(tool_result_id));
        self.abort_active_batch();
    }

    fn check_timeout(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.started_at.elapsed() <= Duration::from_secs(30) {
            self.pending = Some(pending);
            return;
        }
        let llm_request_id = pending.llm_request_id.clone();
        let tool_result_id = Uuid::new_v4().to_string();
        self.emit_tool_result(&pending, tool_result_id.clone(), serde_json::json!({ "error": "timeout" }), false);
        self.mark_batch_completion(llm_request_id.as_deref(), false);
        self.emit_acted(pending, String::new(), "timeout".to_string(), None, 30_000, false, Some(tool_result_id));
        self.abort_active_batch();
    }

    fn emit_acted(&mut self, pending: PendingAct, stdout: String, stderr: String, exit_code: Option<i32>, duration_ms: u64, success: bool, tool_result_id: Option<String>) {
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

    fn emit_missing_args(&mut self, planned: &LoopPlanned, reason: &str) {
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
        self.mark_batch_inline_completion(planned, false);
    }

    fn emit_tool_result(&self, pending: &PendingAct, tool_result_id: String, output: Value, success: bool) {
        if let Some(emitter) = self.emitter.as_ref() {
            self.write_tool_result_artifact(pending.artifact_n, pending, &tool_result_id, &output, success);
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

    fn next_tool_artifact_n(&mut self) -> u32 {
        let n = self.artifact_counter;
        self.artifact_counter = self.artifact_counter.saturating_add(1);
        n
    }

    fn artifact_index_for_plan(&mut self, planned: &LoopPlanned) -> u32 {
        if let Some(request_id) = planned.llm_request_id.as_deref() {
            if let Some(n) = find_request_index_by_request_id(&self.artifact_dir, request_id) {
                if let Some(cache_key) = plan_cache_key(planned) {
                    self.queued_artifact_index.insert(cache_key, n);
                }
                return n;
            }
        }
        if let Some(cache_key) = plan_cache_key(planned) {
            if let Some(n) = self.queued_artifact_index.get(&cache_key) {
                return *n;
            }
            let n = self.next_tool_artifact_n();
            self.queued_artifact_index.insert(cache_key, n);
            return n;
        }
        self.next_tool_artifact_n()
    }

    fn clear_cached_artifact_index_for_plan(&mut self, planned: &LoopPlanned) {
        if let Some(cache_key) = plan_cache_key(planned) {
            self.queued_artifact_index.remove(&cache_key);
        }
    }

    fn write_tool_call_queued_artifact(&self, artifact_n: u32, planned: &LoopPlanned) {
        let value = serde_json::json!({
            "n": artifact_n,
            "status": "queued",
            "queued_ms": now_ms_u64(),
            "action_kind": planned.action_kind,
            "llm_request_id": planned.llm_request_id,
            "plan_id": planned.plan_id,
            "plan_step_id": planned.plan_step_id,
            "action_id": planned.action_id,
            "payload": planned.action_payload,
        });
        append_tool_artifact(&self.artifact_dir, artifact_n, "tool_call", &value);
    }

    fn write_tool_call_artifact(&self, artifact_n: u32, kind: &str, node_id: &str, tool_call_id: &str, request_id: &str, payload: &Value) {
        let value = serde_json::json!({
            "n": artifact_n,
            "status": "dispatched",
            "dispatched_ms": now_ms_u64(),
            "action_id": node_id,
            "kind": kind,
            "node_id": node_id,
            "tool_call_id": tool_call_id,
            "request_id": request_id,
            "payload": payload,
        });
        append_tool_artifact(&self.artifact_dir, artifact_n, "tool_call", &value);
    }

    fn write_tool_result_pending_artifact(&self, artifact_n: u32, planned: &LoopPlanned, kind: &str, node_id: &str, tool_call_id: &str, request_id: &str) {
        let value = serde_json::json!({
            "n": artifact_n,
            "status": "pending",
            "dispatched_ms": now_ms_u64(),
            "tick": planned.tick,
            "action_kind": planned.action_kind,
            "trace_id": planned.trace_id,
            "execution_id": planned.execution_id,
            "parent_span_id": planned.span_id,
            "plan_id": planned.plan_id,
            "plan_step_id": planned.plan_step_id,
            "action_id": planned.action_id,
            "llm_request_id": planned.llm_request_id,
            "kind": kind,
            "node_id": node_id,
            "tool_call_id": tool_call_id,
            "request_id": request_id,
            "success": false,
            "output": {"status":"pending"}
        });
        upsert_tool_result_artifact(&self.artifact_dir, artifact_n, &value);
    }

    fn write_tool_result_artifact(&self, artifact_n: u32, pending: &PendingAct, tool_result_id: &str, output: &Value, success: bool) {
        let status = if success { "completed" } else { "failed" };
        let value = serde_json::json!({
            "n": artifact_n,
            "status": status,
            "dispatched_ms": now_ms_u64(),
            "finalized_ms": now_ms_u64(),
            "tick": pending.tick,
            "action_kind": pending.action_kind,
            "trace_id": pending.trace_id,
            "execution_id": pending.execution_id,
            "parent_span_id": pending.parent_span_id,
            "plan_id": pending.plan_id,
            "plan_step_id": pending.plan_step_id,
            "action_id": pending.action_id,
            "kind": pending.tool_kind,
            "node_id": pending.node_id,
            "tool_call_id": pending.tool_call_id,
            "tool_result_id": tool_result_id,
            "request_id": pending.request_id,
            "success": success,
            "output": output,
        });
        upsert_tool_result_artifact(&self.artifact_dir, artifact_n, &value);
    }

    fn reconcile_stale_pending_artifacts(&mut self) {
        if self.last_reconcile.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
            return;
        }
        self.last_reconcile = Some(Instant::now());

        let timeout_ms = env::var("CANON_TOOL_PENDING_TIMEOUT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(5_000);
        let now_ms = now_ms_u64();
        let Ok(entries) = std::fs::read_dir(&self.artifact_dir) else {
            return;
        };
        let mut any_changed = false;

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with("_tool_results.json") {
                continue;
            }
            let rows = read_artifact_rows(&path);
            if rows.is_empty() {
                continue;
            }

            let mut changed = false;
            let mut out_rows = rows;
            for row in &mut out_rows {
                if row.get("status").and_then(|v| v.as_str()) != Some("pending") {
                    continue;
                }
                let dispatched_ms = row.get("dispatched_ms").and_then(|v| v.as_u64()).unwrap_or(now_ms);
                if now_ms.saturating_sub(dispatched_ms) < timeout_ms {
                    continue;
                }

                let tool_call_id = row.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let request_id = row.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let node_id = row.get("node_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if tool_call_id.is_empty() || request_id.is_empty() || kind.is_empty() || node_id.is_empty() {
                    continue;
                }

                let error = "aborted_or_timeout";
                let tool_result_id = Uuid::new_v4().to_string();
                row["status"] = Value::String("failed".to_string());
                row["finalized_ms"] = Value::from(now_ms);
                row["tool_result_id"] = Value::String(tool_result_id.clone());
                row["success"] = Value::Bool(false);
                row["output"] = serde_json::json!({"error": error});
                changed = true;

                if let Some(emitter) = self.emitter.as_ref() {
                    emitter.emit(CanonEvent::ToolResult(ToolResult {
                        node_id: node_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_result_id: tool_result_id.clone(),
                        request_id: request_id.clone(),
                        kind: kind.clone(),
                        output: serde_json::json!({"error": error}),
                        success: false,
                    }));
                    emitter.emit(CanonEvent::LoopActed(LoopActed {
                        tick: row.get("tick").and_then(|v| v.as_u64()).unwrap_or(0),
                        action_kind: row.get("action_kind").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
                        capability_request_id: request_id,
                        tool_call_id: Some(tool_call_id),
                        tool_result_id: Some(tool_result_id),
                        stdout: String::new(),
                        stderr: error.to_string(),
                        exit_code: None,
                        duration_ms: now_ms.saturating_sub(dispatched_ms),
                        success: false,
                        trace_id: row.get("trace_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        execution_id: row.get("execution_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        span_id: Some(Uuid::new_v4().to_string()),
                        parent_span_id: row.get("parent_span_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        plan_id: row.get("plan_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        plan_step_id: row.get("plan_step_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        action_id: row.get("action_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    }));
                }

                self.mark_batch_completion(row.get("llm_request_id").and_then(|v| v.as_str()), false);
            }

            if changed {
                let serialized = serde_json::to_string_pretty(&out_rows).unwrap_or_default();
                write_atomic(&path, &serialized);
                any_changed = true;
            }
        }
        if any_changed {
            self.dispatch_next_in_active_batch();
        }
    }

    fn mark_batch_planned(&mut self, planned: &LoopPlanned, artifact_n: u32) {
        let Some(llm_request_id) = planned.llm_request_id.clone() else {
            return;
        };
        let snapshot = {
            let status = self.batch_tracker.entry(llm_request_id.clone()).or_insert_with(|| BatchStatus { artifact_n, ..BatchStatus::default() });
            if status.artifact_n == 0 {
                status.artifact_n = artifact_n;
            }
            status.planned = status.planned.saturating_add(1);
            status.clone()
        };
        self.write_batch_status_artifact(snapshot.artifact_n, &llm_request_id, "in_progress", &snapshot);
    }

    fn mark_batch_dispatched(&mut self, planned: &LoopPlanned) {
        let Some(llm_request_id) = planned.llm_request_id.as_deref() else {
            return;
        };
        let Some(status) = self.batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        status.dispatched = status.dispatched.saturating_add(1);
        let snapshot = status.clone();
        self.write_batch_status_artifact(snapshot.artifact_n, llm_request_id, "in_progress", &snapshot);
    }

    fn mark_batch_inline_completion(&mut self, planned: &LoopPlanned, success: bool) {
        self.mark_batch_completion(planned.llm_request_id.as_deref(), success);
    }

    fn mark_batch_completion(&mut self, llm_request_id: Option<&str>, success: bool) {
        let Some(llm_request_id) = llm_request_id else {
            return;
        };
        let mut should_remove = false;
        let Some(status) = self.batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        if success {
            status.completed_ok = status.completed_ok.saturating_add(1);
        } else {
            status.completed_fail = status.completed_fail.saturating_add(1);
        }
        let finished = status.completed_ok.saturating_add(status.completed_fail) >= status.planned;
        let status_label = if finished {
            if status.completed_fail == 0 {
                "completed"
            } else {
                "failed_partial"
            }
        } else {
            "in_progress"
        };
        let snapshot = status.clone();
        self.write_batch_status_artifact(snapshot.artifact_n, llm_request_id, status_label, &snapshot);
        if finished {
            should_remove = true;
        }
        if should_remove {
            self.batch_tracker.remove(llm_request_id);
        }
    }

    fn write_batch_status_artifact(&self, artifact_n: u32, llm_request_id: &str, status: &str, batch: &BatchStatus) {
        let _ = std::fs::create_dir_all(&self.artifact_dir);
        let path = artifact_path_for(&self.artifact_dir, artifact_n, "batch_status");
        let value = serde_json::json!({
            "n": artifact_n,
            "llm_request_id": llm_request_id,
            "status": status,
            "planned": batch.planned,
            "dispatched": batch.dispatched,
            "completed_ok": batch.completed_ok,
            "completed_fail": batch.completed_fail,
            "updated_ms": now_ms_u64(),
        });
        let serialized = serde_json::to_string_pretty(&value).unwrap_or_default();
        write_atomic(&path, &serialized);
    }

    fn dispatch_batch_on_execute(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let Some(first) = self.queue.pop_front() else {
            self.active_batch_llm_request_id = None;
            return;
        };
        self.active_batch_llm_request_id = first.llm_request_id.clone();
        self.dispatch_plan(&first);
        self.dispatch_next_in_active_batch();
    }

    fn dispatch_next_in_active_batch(&mut self) {
        while self.pending.is_none() {
            let Some(next) = self.queue.front() else {
                self.active_batch_llm_request_id = None;
                break;
            };
            let same_batch = next.llm_request_id == self.active_batch_llm_request_id;
            if !same_batch {
                break;
            }
            let next = self.queue.pop_front().expect("front exists");
            self.dispatch_plan(&next);
        }
    }

    fn abort_active_batch(&mut self) {
        loop {
            let Some(next) = self.queue.front() else {
                break;
            };
            let same_batch = next.llm_request_id == self.active_batch_llm_request_id;
            if !same_batch {
                break;
            }
            let next = self.queue.pop_front().expect("front exists");
            self.mark_batch_inline_completion(&next, false);
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit(CanonEvent::LoopActed(LoopActed {
                    tick: next.tick,
                    action_kind: next.action_kind.clone(),
                    capability_request_id: String::new(),
                    tool_call_id: None,
                    tool_result_id: None,
                    stdout: String::new(),
                    stderr: "skipped:batch_aborted".to_string(),
                    exit_code: None,
                    duration_ms: 0,
                    success: false,
                    trace_id: next.trace_id.clone(),
                    execution_id: next.execution_id.clone(),
                    span_id: Some(Uuid::new_v4().to_string()),
                    parent_span_id: next.span_id.clone(),
                    plan_id: next.plan_id.clone(),
                    plan_step_id: next.plan_step_id.clone(),
                    action_id: next.action_id.clone(),
                }));
            }
        }
        self.active_batch_llm_request_id = None;
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

fn plan_cache_key(planned: &LoopPlanned) -> Option<String> {
    planned.action_id.as_ref().map(|id| format!("action:{id}")).or_else(|| planned.plan_step_id.as_ref().map(|id| format!("step:{id}")))
}

fn is_potentially_destructive(cmd: &str, workspace: &Path) -> bool {
    let trimmed = cmd.trim();
    if trimmed.contains("rm -rf") || trimmed.contains("rm -fr") || trimmed.contains("rm -r ") || trimmed.contains("rm -f ") {
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let target = parts.iter().rev().find(|part| !part.starts_with('-')).copied().unwrap_or("");
        let target_path = Path::new(target);
        let resolved_target = if target_path.is_absolute() { target_path.to_path_buf() } else { workspace.join(target_path) };
        if resolved_target.starts_with(workspace) {
            return false;
        }
        return true;
    }
    if trimmed.contains("git reset --hard") || trimmed.contains("git clean -f") {
        return true;
    }
    if trimmed.starts_with("dd ") || trimmed.starts_with("mkfs") || trimmed.starts_with("shred ") {
        return true;
    }
    false
}

fn extract_result_fields(result: &Value, started_at: Instant) -> (String, String, Option<i32>, u64, bool) {
    let stdout = result.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let stderr = result.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let exit_code = result.get("status").and_then(|v| v.as_i64()).map(|v| v as i32).or_else(|| result.get("exit_code").and_then(|v| v.as_i64()).map(|v| v as i32));
    let duration_ms = result.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or_else(|| started_at.elapsed().as_millis() as u64);
    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
    (stdout, stderr, exit_code, duration_ms, success)
}

fn default_tool_artifact_dir() -> PathBuf {
    PathBuf::from(env::var("CANON_LLM_LOG_DIR").unwrap_or_else(|_| "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm".to_string()))
}

fn next_tool_artifact_counter(log_dir: &Path) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((n, _suffix, _ts)) = parse_artifact_name(&name) else {
            continue;
        };
        max_seen = Some(max_seen.map_or(n, |m| m.max(n)));
    }
    max_seen.map_or(0, |m| m.saturating_add(1))
}

fn append_tool_artifact(log_dir: &Path, artifact_n: u32, suffix: &str, value: &Value) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = artifact_path_for(log_dir, artifact_n, suffix);
    let mut rows = read_artifact_rows(&path);
    rows.push(value.clone());
    let serialized = serde_json::to_string_pretty(&rows).unwrap_or_default();
    write_atomic(&path, &serialized);
}

fn upsert_tool_result_artifact(log_dir: &Path, artifact_n: u32, value: &Value) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = artifact_path_for(log_dir, artifact_n, "tool_results");
    let mut rows = read_artifact_rows(&path);
    let key = value.get("tool_call_id").and_then(|v| v.as_str());
    let mut replaced = false;
    if let Some(key) = key {
        for row in &mut rows {
            if row.get("tool_call_id").and_then(|v| v.as_str()) == Some(key) {
                *row = value.clone();
                replaced = true;
                break;
            }
        }
    }
    if !replaced {
        rows.push(value.clone());
    }
    let serialized = serde_json::to_string_pretty(&rows).unwrap_or_default();
    write_atomic(&path, &serialized);
}

fn write_atomic(path: &Path, content: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

fn read_artifact_rows(path: &Path) -> Vec<Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    match parsed {
        Value::Array(arr) => arr,
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn find_request_index_by_request_id(log_dir: &Path, request_id: &str) -> Option<u32> {
    let entries = std::fs::read_dir(log_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_str()?;
        let Some((n, suffix, _ts)) = parse_artifact_name(name) else {
            continue;
        };
        if suffix != "request.json" {
            continue;
        }
        let raw = std::fs::read_to_string(entry.path()).ok()?;
        let v = serde_json::from_str::<Value>(&raw).ok()?;
        if v.get("request_id").and_then(|x| x.as_str()) == Some(request_id) {
            return Some(n);
        }
    }
    None
}

fn now_ms_u64() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn parse_artifact_name(name: &str) -> Option<(u32, String, Option<u64>)> {
    let mut parts = name.splitn(3, '_');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next()?;

    if let Ok(n) = first.parse::<u32>() {
        // Legacy: <REQNUM>_<suffix>.json
        return Some((n, format!("{}_{}", second, third), None));
    }
    // New: <TS>_<REQNUM>_<suffix>.json
    let ts = first.parse::<u64>().ok()?;
    let n = second.parse::<u32>().ok()?;
    Some((n, third.to_string(), Some(ts)))
}

fn resolve_artifact_ts(log_dir: &Path, artifact_n: u32) -> u64 {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return now_ms_u64();
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((n, _suffix, ts)) = parse_artifact_name(&name) else {
            continue;
        };
        if n == artifact_n {
            if let Some(ts) = ts {
                return ts;
            }
        }
    }
    now_ms_u64()
}

fn artifact_path_for(log_dir: &Path, artifact_n: u32, suffix: &str) -> PathBuf {
    let ts = resolve_artifact_ts(log_dir, artifact_n);
    log_dir.join(format!("{ts}_{artifact_n:04}_{suffix}.json"))
}
