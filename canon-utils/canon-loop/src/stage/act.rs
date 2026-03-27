use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use canon_editor::rename_symbol_pairs;
use canon_event::{
    new_error_occurred, BashInvoke, CapabilityCompleted, CapabilityFailed, CapabilityResult, EventId, FileEvent, FilePatch, FileWrite, LoopActed, LoopPlanned, ProcessResult, RouteSelected, RuntimeEvent, ToolCall, ToolResult,
};
use canon_goal::parse_agent_goal_markdown;
use canon_tools_patch::apply_patch;
use canon_invariant::decision_trace_payload;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    context::{DestructiveCmdPolicy, LoopContext, PendingAct},
    merge::extract_written_paths,
    result::LoopStageResult,
};

pub fn execute_dispatch(_rs: RouteSelected, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_act.is_some() {
        emit_act_stall(
            ctx,
            &trigger_id,
            "pending_act already present; cannot dispatch a new action",
        );
        return Ok(LoopStageResult::Noop);
    }
    if ctx.scheduler.is_empty() {
        ctx.active_batch_llm_request_id = None;
        emit_act_stall(
            ctx,
            &trigger_id,
            "route_selected(act) but scheduler is empty",
        );
        return Ok(LoopStageResult::Noop);
    }
    let filtered_batch_id = ctx.active_batch_llm_request_id.clone();
    let scheduler_len_before = ctx.scheduler.len();
    let task = if let Some(batch_id) = filtered_batch_id.as_deref() {
        ctx.scheduler.pop_for_llm(Some(batch_id))
    } else {
        ctx.scheduler.pop_any()
    };
    let task = match task {
        Some(task) => task,
        None => {
            if scheduler_len_before > 0 && filtered_batch_id.is_some() {
                // A stale batch affinity can strand otherwise dispatchable work from a newer plan.
                ctx.active_batch_llm_request_id = None;
                if let Some(task) = ctx.scheduler.pop_any() {
                    task
                } else {
                    emit_act_stall_with_context(
                        ctx,
                        &trigger_id,
                        "scheduler has queued work but no dispatchable task was returned",
                        serde_json::json!({
                            "scheduler_len": scheduler_len_before,
                            "filtered_batch_id": filtered_batch_id,
                            "dispatch_mode": "pop_for_llm_then_pop_any",
                        }),
                    );
                    return Ok(LoopStageResult::Noop);
                }
            } else {
                if scheduler_len_before > 0 {
                    emit_act_stall_with_context(
                        ctx,
                        &trigger_id,
                        "scheduler has queued work but no dispatchable task was returned",
                        serde_json::json!({
                            "scheduler_len": scheduler_len_before,
                            "filtered_batch_id": filtered_batch_id,
                            "dispatch_mode": if filtered_batch_id.is_some() { "pop_for_llm" } else { "pop_any" },
                        }),
                    );
                }
                ctx.active_batch_llm_request_id = None;
                return Ok(LoopStageResult::Noop);
            }
        }
    };
    ctx.active_batch_llm_request_id = task.plan.llm_request_id.clone();
    dispatch_plan(ctx, &task.plan, &trigger_id)
}

fn emit_act_stall(ctx: &LoopContext, trigger_id: &EventId, reason: &str) {
    emit_act_stall_with_context(ctx, trigger_id, reason, serde_json::json!({}));
}

fn emit_act_stall_with_context(
    ctx: &LoopContext,
    trigger_id: &EventId,
    reason: &str,
    extra: serde_json::Value,
) {
    let Some(emitter) = ctx.emitter.as_ref() else {
        return;
    };
    let mut payload = serde_json::json!({
        "reason": reason,
        "scheduler_len": ctx.scheduler.len(),
        "pending_act_present": ctx.pending_act.is_some(),
        "active_batch_llm_request_id": ctx.active_batch_llm_request_id,
        "last_action_kind": ctx.last_action_kind,
    });
    if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    emitter.emit_child(
        RuntimeEvent::Debug(canon_event::DebugEvent {
            source: "act_stage".to_string(),
            kind: "act_suppressed".to_string(),
            payload: decision_trace_payload("act dispatch returned noop", payload.clone()),
        }),
        vec![trigger_id.clone()],
        file!(),
        line!(),
    );
    emitter.emit_child(
        RuntimeEvent::ErrorOccurred(new_error_occurred(
            "act_stall",
            "act_stage",
            reason.to_string(),
            "warning",
            payload,
            None,
        )),
        vec![trigger_id.clone()],
        file!(),
        line!(),
    );
}

pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext, _trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_act.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != c.request_id {
        ctx.pending_act = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    let (stdout, stderr, exit_code, duration_ms, success) = extract_result_fields(&c.result, pending.started_at);
    let action_kind = pending.action_kind.clone();
    let llm_request_id = pending.llm_request_id.clone();
    let tool_result_id = Uuid::new_v4().to_string();
    let mut events = Vec::new();
    events.push(emit_tool_result(ctx, &pending, tool_result_id.clone(), c.result.clone(), success));
    ctx.mark_batch_completion(llm_request_id.as_deref(), success);
    if ctx.pending_required_successor.as_deref() != Some("loop_acted") {
        events.push(RuntimeEvent::Debug(canon_event::DebugEvent {
            source: "act_stage".to_string(),
            kind: "act_suppressed".to_string(),
            payload: decision_trace_payload(
                "act completion suppressed because control FSM expects a different successor",
                serde_json::json!({
                    "reason": "loop_acted would violate pending successor",
                    "pending_required_successor": ctx.pending_required_successor,
                    "last_control_kind": ctx.last_control_kind,
                    "last_control_event_id": ctx.last_control_event_id,
                    "action_kind": action_kind,
                }),
            ),
        }));
        events.push(RuntimeEvent::ErrorOccurred(new_error_occurred(
            "act_stall",
            "act_stage",
            "act completion arrived after control moved past loop_acted".to_string(),
            "warning",
            serde_json::json!({
                "pending_required_successor": ctx.pending_required_successor,
                "last_control_kind": ctx.last_control_kind,
                "last_control_event_id": ctx.last_control_event_id,
                "recoverable": true,
            }),
            None,
        )));
        events.extend(abort_active_batch(ctx));
        return Ok(LoopStageResult::EmitMany(events));
    }
    let acted_event = emit_acted(pending, stdout, stderr, exit_code, duration_ms, success, Some(tool_result_id));
    events.push(acted_event);

    if !success && action_kind == "run_command" {
        events.extend(abort_active_batch(ctx));
    } else if ctx.pending_act.is_none() {
        // One route_selected(act) should produce one control successor (loop_acted).
        // Do not auto-dispatch more actions here; let routing select act again if work remains.
        ctx.active_batch_llm_request_id = None;
    }
    Ok(LoopStageResult::EmitMany(events))
}

pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext, _trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_act.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != f.request_id {
        ctx.pending_act = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    let duration_ms = pending.started_at.elapsed().as_millis() as u64;
    let action_kind = pending.action_kind.clone();
    let llm_request_id = pending.llm_request_id.clone();
    let tool_result_id = Uuid::new_v4().to_string();
    let mut events = Vec::new();
    events.push(emit_tool_result(
        ctx,
        &pending,
        tool_result_id.clone(),
        CapabilityResult::Process(ProcessResult { status: -1, success: false, stdout: String::new(), stderr: f.error.clone() }),
        false,
    ));
    ctx.mark_batch_completion(llm_request_id.as_deref(), false);
    if ctx.pending_required_successor.as_deref() != Some("loop_acted") {
        events.push(RuntimeEvent::Debug(canon_event::DebugEvent {
            source: "act_stage".to_string(),
            kind: "act_suppressed".to_string(),
            payload: decision_trace_payload(
                "failed act completion suppressed because control FSM expects a different successor",
                serde_json::json!({
                    "reason": "loop_acted would violate pending successor",
                    "pending_required_successor": ctx.pending_required_successor,
                    "last_control_kind": ctx.last_control_kind,
                    "last_control_event_id": ctx.last_control_event_id,
                    "action_kind": action_kind,
                }),
            ),
        }));
        events.push(RuntimeEvent::ErrorOccurred(new_error_occurred(
            "act_stall",
            "act_stage",
            "failed act completion arrived after control moved past loop_acted".to_string(),
            "warning",
            serde_json::json!({
                "pending_required_successor": ctx.pending_required_successor,
                "last_control_kind": ctx.last_control_kind,
                "last_control_event_id": ctx.last_control_event_id,
                "recoverable": true,
            }),
            None,
        )));
        events.extend(abort_active_batch(ctx));
        return Ok(LoopStageResult::EmitMany(events));
    }
    events.push(emit_acted(pending, String::new(), f.error.clone(), None, duration_ms, false, Some(tool_result_id)));
    if action_kind == "run_command" {
        events.extend(abort_active_batch(ctx));
    } else if ctx.pending_act.is_none() {
        ctx.active_batch_llm_request_id = None;
    }
    Ok(LoopStageResult::EmitMany(events))
}

// -------------------------------------------------------------------------------------
// Dispatch helpers (ported from ActConsumer)
// -------------------------------------------------------------------------------------

fn dispatch_plan(ctx: &mut LoopContext, planned: &LoopPlanned, trigger_id: &EventId) -> anyhow::Result<LoopStageResult> {
    match planned.action_kind.as_str() {
        "no_op" | "done" => {
            ctx.mark_batch_inline_completion(planned, true);
            Ok(LoopStageResult::Emit(emit_acted(
                PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: String::new(),
                    request_id: String::new(),
                    tool_call_id: String::new(),
                    node_id: String::new(),
                    started_at: Instant::now(),
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                    artifact_n: 0,
                    llm_request_id: planned.llm_request_id.clone(),
                },
                String::new(),
                String::new(),
                None,
                0,
                true,
                None,
            )))
        }
        "run_command" => {
            let cmd = planned.action_payload.get("cmd").and_then(|v| v.as_str());
            let cwd = planned.action_payload.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");
            let Some(cmd) = cmd else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_cmd")));
            };
            if is_potentially_destructive(cmd, &ctx.workspace) {
                match ctx.destructive_cmd_policy {
                    DestructiveCmdPolicy::Block => {
                        ctx.mark_batch_inline_completion(planned, false);
                        return Ok(LoopStageResult::Emit(emit_missing_args(planned, "rejected_destructive_command")));
                    }
                    DestructiveCmdPolicy::Warn => {
                        // emit warning
                        if let Some(emitter) = ctx.emitter.as_ref() {
                            emitter.emit_with_parents(canon_event::RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "act_consumer".to_string(),
                                kind: "destructive_command_warning".to_string(),
                                payload: serde_json::json!({
                                    "cmd": cmd,
                                    "policy": ctx.destructive_cmd_policy.as_str(),
                                    "action_id": planned.action_id,
                                }),
                            }), vec![trigger_id.clone()], file!(), line!());
                        }
                    }
                    DestructiveCmdPolicy::Allow => {}
                }
            }
            let request_id = Uuid::new_v4().to_string();
            let tool_call_id = Uuid::new_v4().to_string();
            let node_id = tool_node_id(planned);
            let artifact_n = ctx.artifact_index_for_plan(planned);
            ctx.clear_cached_artifact_index_for_plan(planned);
            ctx.mark_batch_dispatched(planned);

            let mut events = Vec::new();
            events.push(write_tool_call_artifact(ctx, artifact_n, "bash", &node_id, &tool_call_id, &request_id, &serde_json::json!({ "cmd": cmd, "cwd": cwd })));
            events.push(write_tool_result_pending_artifact(ctx, artifact_n, planned, "bash", &node_id, &tool_call_id, &request_id));
            events.push(RuntimeEvent::ToolCall(ToolCall {
                node_id: node_id.clone(),
                tool_call_id: tool_call_id.clone(),
                request_id: request_id.clone(),
                kind: "bash".to_string(),
                payload: serde_json::json!({ "cmd": cmd, "cwd": cwd }),
                accepted: true,
            }));
            events.push(RuntimeEvent::Bash(BashInvoke { request_id: request_id.clone(), cmd: cmd.to_string(), cwd: Some(cwd.to_string()), queued: true }));

            ctx.pending_act = Some(PendingAct {
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
            Ok(LoopStageResult::EmitMany(events))
        }
        "write_file" => {
            let path = planned.action_payload.get("path").and_then(|v| v.as_str());
            let content = planned.action_payload.get("content").and_then(|v| v.as_str());
            let (Some(path), Some(content)) = (path, content) else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_path_or_content")));
            };
            let resolved = resolve_action_path(path, ctx);
            let action_key = planned.action_id.clone().unwrap_or_else(|| tool_node_id(planned));
            if let Some((agent, action)) = ctx.file_write_tracker.claim(&resolved, "orchestrator", &action_key) {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_conflict(planned, &agent, &action, resolved.to_string_lossy().as_ref())));
            }
            ctx.write_paths_by_action.insert(action_key.clone(), vec![resolved.clone()]);
            let request_id = Uuid::new_v4().to_string();
            let tool_call_id = Uuid::new_v4().to_string();
            let node_id = tool_node_id(planned);
            let artifact_n = ctx.artifact_index_for_plan(planned);
            ctx.clear_cached_artifact_index_for_plan(planned);
            ctx.mark_batch_dispatched(planned);

            let mut events = Vec::new();
            events.push(write_tool_call_artifact(ctx, artifact_n, "file.write", &node_id, &tool_call_id, &request_id, &serde_json::json!({ "path": path, "content": content })));
            events.push(write_tool_result_pending_artifact(ctx, artifact_n, planned, "file.write", &node_id, &tool_call_id, &request_id));
            events.push(RuntimeEvent::ToolCall(ToolCall {
                node_id: node_id.clone(),
                tool_call_id: tool_call_id.clone(),
                request_id: request_id.clone(),
                kind: "file.write".to_string(),
                payload: serde_json::json!({ "path": path, "content": content }),
                accepted: true,
            }));
            events.push(RuntimeEvent::File(FileEvent::Write(FileWrite { request_id: request_id.clone(), path: path.to_string(), content: content.to_string(), queued: true })));

            ctx.pending_act = Some(PendingAct {
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
            Ok(LoopStageResult::EmitMany(events))
        }
        "patch_file" => {
            let path = planned.action_payload.get("path").and_then(|v| v.as_str());
            let old = planned.action_payload.get("old").and_then(|v| v.as_str());
            let new = planned.action_payload.get("new").and_then(|v| v.as_str());
            let (Some(path), Some(old), Some(new)) = (path, old, new) else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_patch_args")));
            };
            let request_id = Uuid::new_v4().to_string();
            let tool_call_id = Uuid::new_v4().to_string();
            let node_id = tool_node_id(planned);
            let artifact_n = ctx.artifact_index_for_plan(planned);
            ctx.clear_cached_artifact_index_for_plan(planned);
            ctx.mark_batch_dispatched(planned);

            let mut events = Vec::new();
            events.push(write_tool_call_artifact(ctx, artifact_n, "file.patch", &node_id, &tool_call_id, &request_id, &serde_json::json!({ "path": path, "old": old, "new": new })));
            events.push(write_tool_result_pending_artifact(ctx, artifact_n, planned, "file.patch", &node_id, &tool_call_id, &request_id));
            events.push(RuntimeEvent::ToolCall(ToolCall {
                node_id: node_id.clone(),
                tool_call_id: tool_call_id.clone(),
                request_id: request_id.clone(),
                kind: "file.patch".to_string(),
                payload: serde_json::json!({ "path": path, "old": old, "new": new }),
                accepted: true,
            }));
            events.push(RuntimeEvent::File(FileEvent::Patch(FilePatch { request_id: request_id.clone(), path: path.to_string(), old: old.to_string(), new: new.to_string(), queued: true })));

            ctx.pending_act = Some(PendingAct {
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
            Ok(LoopStageResult::EmitMany(events))
        }
        "apply_patch" => {
            let patch = planned.action_payload.get("patch").and_then(|v| v.as_str());
            let Some(patch) = patch else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_patch_body")));
            };
            let node_id = tool_node_id(planned);
            let patch_cwd = ctx.goal_text.as_deref().and_then(|t| parse_agent_goal_markdown(t).target_path).unwrap_or_else(|| ctx.workspace.clone());
            let touched_paths: Vec<_> = extract_written_paths("apply_patch", &planned.action_payload).into_iter().map(|p| if p.is_absolute() { p } else { patch_cwd.join(p) }).collect();
            let action_key = planned.action_id.clone().unwrap_or_else(|| tool_node_id(planned));
            for path in &touched_paths {
                if let Some((agent, action)) = ctx.file_write_tracker.claim(path, "orchestrator", &action_key) {
                    ctx.mark_batch_inline_completion(planned, false);
                    return Ok(LoopStageResult::Emit(emit_conflict(planned, &agent, &action, path.to_string_lossy().as_ref())));
                }
            }
            if !touched_paths.is_empty() {
                ctx.write_paths_by_action.insert(action_key.clone(), touched_paths.clone());
            }
            let started = Instant::now();
            std::fs::create_dir_all(&patch_cwd).ok();
            let result = apply_patch(patch, &patch_cwd);
            let duration_ms = started.elapsed().as_millis() as u64;
            let success = result.is_ok();
            let stdout = match &result {
                Ok(affected) => format!("apply_patch ok: added {} modified {} deleted {}", affected.added.len(), affected.modified.len(), affected.deleted.len()),
                Err(err) => format!("apply_patch failed: {err}"),
            };
            ctx.mark_batch_inline_completion(planned, success);
            let tool_result = inline_tool_result(
                "apply_patch",
                &node_id,
                serde_json::json!({
                    "stdout": stdout,
                    "stderr": "",
                    "duration_ms": duration_ms,
                    "touched_paths": touched_paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                    "success": success,
                }),
                success,
            );
            let acted = emit_acted(
                PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "apply_patch".to_string(),
                    request_id: String::new(),
                    tool_call_id: String::new(),
                    node_id: node_id.clone(),
                    started_at: started,
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                    artifact_n: 0,
                    llm_request_id: planned.llm_request_id.clone(),
                },
                stdout,
                String::new(),
                None,
                duration_ms,
                success,
                None,
            );
            Ok(LoopStageResult::EmitMany(vec![tool_result, acted]))
        }
        "edit.rename_symbol" => {
            let old = planned.action_payload.get("old").and_then(|v| v.as_str());
            let new = planned.action_payload.get("new").and_then(|v| v.as_str());
            let (Some(old), Some(new)) = (old, new) else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_rename_args")));
            };
            let project = planned
                .action_payload
                .get("project")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| ctx.workspace.clone());
            let node_id = tool_node_id(planned);
            let started = Instant::now();
            let report = rename_symbol_pairs(&project, &[(old.to_string(), new.to_string())]);
            let duration_ms = started.elapsed().as_millis() as u64;
            let success = report.error.is_none();
            let stdout = if success {
                if report.def_paths.is_empty() {
                    format!("rename_symbol ok: {old} -> {new}")
                } else {
                    format!("rename_symbol ok: {old} -> {new}\n{}", report.def_paths.join("\n"))
                }
            } else {
                String::new()
            };
            let stderr = report.error.unwrap_or_default();
            ctx.mark_batch_inline_completion(planned, success);
            let tool_result = inline_tool_result(
                "edit.rename_symbol",
                &node_id,
                serde_json::json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "duration_ms": duration_ms,
                    "success": success,
                    "old": old,
                    "new": new,
                }),
                success,
            );
            let acted = emit_acted(
                PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "edit.rename_symbol".to_string(),
                    request_id: String::new(),
                    tool_call_id: String::new(),
                    node_id: node_id.clone(),
                    started_at: started,
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                    artifact_n: 0,
                    llm_request_id: planned.llm_request_id.clone(),
                },
                stdout,
                stderr,
                None,
                duration_ms,
                success,
                None,
            );
            Ok(LoopStageResult::EmitMany(vec![tool_result, acted]))
        }
        "read_file" => {
            let path_str = planned.action_payload.get("path").and_then(|v| v.as_str());
            let Some(path_str) = path_str else {
                ctx.mark_batch_inline_completion(planned, false);
                return Ok(LoopStageResult::Emit(emit_missing_args(planned, "missing_path")));
            };
            let path = resolve_action_path(path_str, ctx);
            let node_id = tool_node_id(planned);
            let started = Instant::now();
            let (stdout, success) = match std::fs::read_to_string(&path) {
                Ok(content) => {
                    // Truncate very large files to avoid flooding the context.
                    let content = if content.len() > 8000 { format!("{}\n... <truncated, {} bytes total>", &content[..8000], content.len()) } else { content };
                    (format!("=== {} ===\n{}", path.display(), content), true)
                }
                Err(_) => (String::new(), false),
            };
            let stderr = if !success { format!("read_file failed: {}", path.display()) } else { String::new() };
            let duration_ms = started.elapsed().as_millis() as u64;
            ctx.mark_batch_inline_completion(planned, success);
            let tool_result = inline_tool_result(
                "read_file",
                &node_id,
                serde_json::json!({
                    "path": path.display().to_string(),
                    "stdout": stdout,
                    "stderr": stderr,
                    "duration_ms": duration_ms,
                    "success": success,
                }),
                success,
            );
            let acted = emit_acted(
                PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "read_file".to_string(),
                    request_id: String::new(),
                    tool_call_id: String::new(),
                    node_id: node_id.clone(),
                    started_at: started,
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                    artifact_n: 0,
                    llm_request_id: planned.llm_request_id.clone(),
                },
                stdout,
                stderr,
                None,
                duration_ms,
                success,
                None,
            );
            Ok(LoopStageResult::EmitMany(vec![tool_result, acted]))
        }
        "list_dir" => {
            let path_str = planned.action_payload.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let path = resolve_action_path(path_str, ctx);
            let node_id = tool_node_id(planned);
            let started = Instant::now();
            let (stdout, success) = match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut lines = vec![format!("=== {} ===", path.display())];
                    let mut names: Vec<String> = entries
                        .flatten()
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if e.path().is_dir() {
                                format!("{}/", name)
                            } else {
                                name
                            }
                        })
                        .collect();
                    names.sort();
                    lines.extend(names);
                    (lines.join("\n"), true)
                }
                Err(_) => (String::new(), false),
            };
            let stderr = if !success { format!("list_dir failed: {}", path.display()) } else { String::new() };
            let duration_ms = started.elapsed().as_millis() as u64;
            ctx.mark_batch_inline_completion(planned, success);
            let tool_result = inline_tool_result(
                "list_dir",
                &node_id,
                serde_json::json!({
                    "path": path.display().to_string(),
                    "stdout": stdout,
                    "stderr": stderr,
                    "duration_ms": duration_ms,
                    "success": success,
                }),
                success,
            );
            let acted = emit_acted(
                PendingAct {
                    tick: planned.tick,
                    action_kind: planned.action_kind.clone(),
                    tool_kind: "list_dir".to_string(),
                    request_id: String::new(),
                    tool_call_id: String::new(),
                    node_id: node_id.clone(),
                    started_at: started,
                    trace_id: planned.trace_id.clone(),
                    execution_id: planned.execution_id.clone(),
                    parent_span_id: planned.span_id.clone(),
                    plan_id: planned.plan_id.clone(),
                    plan_step_id: planned.plan_step_id.clone(),
                    action_id: planned.action_id.clone(),
                    artifact_n: 0,
                    llm_request_id: planned.llm_request_id.clone(),
                },
                stdout,
                stderr,
                None,
                duration_ms,
                success,
                None,
            );
            Ok(LoopStageResult::EmitMany(vec![tool_result, acted]))
        }
        _ => {
            ctx.mark_batch_inline_completion(planned, false);
            Ok(LoopStageResult::Emit(emit_missing_args(planned, "unknown_action_kind")))
        }
    }
}

/// Resolve a path string: absolute paths pass through; relative paths resolve
/// against the goal's target_path (or workspace as fallback).
fn resolve_action_path(path_str: &str, ctx: &LoopContext) -> std::path::PathBuf {
    let p = std::path::Path::new(path_str);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let base = ctx.goal_text.as_deref().and_then(|t| parse_agent_goal_markdown(t).target_path).unwrap_or_else(|| ctx.workspace.clone());
    base.join(p)
}

fn emit_missing_args(planned: &LoopPlanned, reason: &str) -> RuntimeEvent {
    RuntimeEvent::LoopActed(LoopActed {
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
    })
}

fn emit_conflict(planned: &LoopPlanned, agent: &str, action: &str, path: &str) -> RuntimeEvent {
    RuntimeEvent::LoopActed(LoopActed {
        tick: planned.tick,
        action_kind: planned.action_kind.clone(),
        capability_request_id: String::new(),
        tool_call_id: None,
        tool_result_id: None,
        stdout: String::new(),
        stderr: format!("conflict: {path} already claimed by agent={agent} action={action}"),
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
    })
}

fn emit_acted(pending: PendingAct, stdout: String, stderr: String, exit_code: Option<i32>, duration_ms: u64, success: bool, tool_result_id: Option<String>) -> RuntimeEvent {
    RuntimeEvent::LoopActed(LoopActed {
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
    })
}

fn emit_tool_result(ctx: &LoopContext, pending: &PendingAct, tool_result_id: String, output: CapabilityResult, success: bool) -> RuntimeEvent {
    let output_json = serde_json::to_value(&output).unwrap_or_else(|_| serde_json::json!({}));
    write_tool_result_artifact(ctx, pending.artifact_n, pending, &tool_result_id, &output_json, success);
    RuntimeEvent::ToolResult(ToolResult {
        node_id: pending.node_id.clone(),
        tool_call_id: pending.tool_call_id.clone(),
        tool_result_id,
        request_id: pending.request_id.clone(),
        kind: pending.tool_kind.clone(),
        output: output_json,
        success,
    })
}

fn inline_tool_result(kind: &str, node_id: &str, output: serde_json::Value, success: bool) -> RuntimeEvent {
    RuntimeEvent::ToolResult(ToolResult {
        node_id: node_id.to_string(),
        tool_call_id: Uuid::new_v4().to_string(),
        tool_result_id: Uuid::new_v4().to_string(),
        request_id: Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        output,
        success,
    })
}

// Artifact helpers (ported)
fn write_tool_call_artifact(ctx: &LoopContext, artifact_n: u32, kind: &str, node_id: &str, tool_call_id: &str, request_id: &str, payload: &Value) -> RuntimeEvent {
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
    append_tool_artifact(&ctx.artifact_dir, artifact_n, "tool_call", &value);
    RuntimeEvent::RuntimeStateUpdated(canon_event::RuntimeStateUpdated { payload: serde_json::json!({"workspace_dirty": false}) })
}

fn write_tool_result_pending_artifact(ctx: &LoopContext, artifact_n: u32, planned: &LoopPlanned, kind: &str, node_id: &str, tool_call_id: &str, request_id: &str) -> RuntimeEvent {
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
    upsert_tool_result_artifact(&ctx.artifact_dir, artifact_n, &value);
    RuntimeEvent::RuntimeStateUpdated(canon_event::RuntimeStateUpdated { payload: serde_json::json!({"workspace_dirty": false}) })
}

fn write_tool_result_artifact(ctx: &LoopContext, artifact_n: u32, pending: &PendingAct, tool_result_id: &str, output: &Value, success: bool) {
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
    upsert_tool_result_artifact(&ctx.artifact_dir, artifact_n, &value);
}

// Batch tracking
fn abort_active_batch(ctx: &mut LoopContext) -> Vec<RuntimeEvent> {
    let mut events = Vec::new();
    while let Some(next) = ctx.active_batch_llm_request_id.as_deref().and_then(|id| ctx.scheduler.pop_for_llm(Some(id))).map(|t| t.plan) {
        ctx.mark_batch_inline_completion(&next, false);
        events.push(emit_missing_args(&next, "skipped:batch_aborted"));
    }
    ctx.active_batch_llm_request_id = None;
    events
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

// Pending timeout/reconcile helpers
pub fn check_act_timeout(ctx: &mut LoopContext) -> Vec<RuntimeEvent> {
    let Some(pending) = ctx.pending_act.take() else {
        return Vec::new();
    };
    if pending.started_at.elapsed() <= Duration::from_secs(30) {
        ctx.pending_act = Some(pending);
        return Vec::new();
    }
    let action_kind = pending.action_kind.clone();
    let llm_request_id = pending.llm_request_id.clone();
    let tool_result_id = Uuid::new_v4().to_string();
    let mut events = Vec::new();
    events.push(emit_tool_result(
        ctx,
        &pending,
        tool_result_id.clone(),
        CapabilityResult::Process(ProcessResult { status: -1, success: false, stdout: String::new(), stderr: "timeout".to_string() }),
        false,
    ));
    ctx.mark_batch_completion(llm_request_id.as_deref(), false);
    events.push(emit_acted(pending, String::new(), "timeout".to_string(), None, 30_000, false, Some(tool_result_id)));
    if action_kind == "run_command" {
        events.extend(abort_active_batch(ctx));
    }
    events.push(RuntimeEvent::LoopActed(LoopActed {
        tick: 0,
        action_kind: "timeout_marker".to_string(),
        capability_request_id: String::new(),
        tool_call_id: None,
        tool_result_id: None,
        stdout: String::new(),
        stderr: String::new(),
        exit_code: None,
        duration_ms: 0,
        success: true,
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
        plan_id: None,
        plan_step_id: None,
        action_id: None,
    }));
    events
}

pub fn reconcile_stale_pending_artifacts(ctx: &mut LoopContext) -> Vec<RuntimeEvent> {
    use std::fs;
    if ctx.last_act_reconcile.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
        return Vec::new();
    }
    ctx.last_act_reconcile = Some(Instant::now());

    let timeout_ms = std::env::var("CANON_TOOL_PENDING_TIMEOUT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(5_000);
    let now_ms = now_ms_u64();
    let Ok(entries) = fs::read_dir(&ctx.artifact_dir) else {
        return Vec::new();
    };
    let mut events = Vec::new();

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

            let tick = row.get("tick").and_then(|v| v.as_u64()).unwrap_or(0);
            events.push(RuntimeEvent::ToolResult(ToolResult {
                node_id: node_id.clone(),
                tool_call_id: tool_call_id.clone(),
                tool_result_id: tool_result_id.clone(),
                request_id: request_id.clone(),
                kind: kind.clone(),
                output: serde_json::json!({"error": error}),
                success: false,
            }));
            events.push(RuntimeEvent::LoopActed(LoopActed {
                tick,
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

            ctx.mark_batch_completion(row.get("llm_request_id").and_then(|v| v.as_str()), false);
        }

        if changed {
            let serialized = serde_json::to_string_pretty(&out_rows).unwrap_or_default();
            write_atomic(&path, &serialized);
        }
    }
    if !events.is_empty() {
        ctx.active_batch_llm_request_id = ctx.scheduler.peek_llm_request_id();
    }
    events
}

// ----- artifact utilities -----

fn now_ms_u64() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn append_tool_artifact(log_dir: &std::path::Path, artifact_n: u32, suffix: &str, value: &Value) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = artifact_path_for(log_dir, artifact_n, suffix);
    let mut rows = read_artifact_rows(&path);
    rows.push(value.clone());
    let serialized = serde_json::to_string_pretty(&rows).unwrap_or_default();
    write_atomic(&path, &serialized);
}

fn upsert_tool_result_artifact(log_dir: &std::path::Path, artifact_n: u32, value: &Value) {
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

fn write_atomic(path: &std::path::Path, content: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

fn read_artifact_rows(path: &std::path::Path) -> Vec<Value> {
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

fn artifact_path_for(log_dir: &std::path::Path, artifact_n: u32, suffix: &str) -> std::path::PathBuf {
    log_dir.join(format!("{:09}_{}.json", artifact_n, suffix))
}

fn is_potentially_destructive(cmd: &str, workspace: &std::path::Path) -> bool {
    let trimmed = cmd.trim();
    if trimmed.contains("rm -rf") || trimmed.contains("rm -fr") || trimmed.contains("rm -r ") || trimmed.contains("rm -f ") {
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let target = parts.iter().rev().find(|part| !part.starts_with('-')).copied().unwrap_or("");
        let target_path = std::path::Path::new(target);
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

fn extract_result_fields(result: &CapabilityResult, started_at: Instant) -> (String, String, Option<i32>, u64, bool) {
    match result {
        CapabilityResult::Process(proc) => {
            let duration_ms = started_at.elapsed().as_millis() as u64;
            (proc.stdout.clone(), proc.stderr.clone(), Some(proc.status), duration_ms, proc.success)
        }
        CapabilityResult::Llm(llm) => {
            let stdout = llm.response.to_string();
            let duration_ms = llm.duration_ms;
            (stdout, String::new(), None, duration_ms, llm.success)
        }
        CapabilityResult::Empty => (String::new(), String::new(), None, started_at.elapsed().as_millis() as u64, true),
    }
}
