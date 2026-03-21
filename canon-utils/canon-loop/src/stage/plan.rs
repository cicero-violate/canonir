use std::path::Path;

use canon_event::{CapabilityCompleted, CapabilityFailed, CapabilityResult, CanonEvent, DebugEvent, LlmCall, LoopActed, LoopObserved, LoopPlanned, ToolCall, ToolResult};
use uuid::Uuid;

use crate::{context::{LoopContext, PendingPlan}, result::LoopStageResult};

const LLM_TIMEOUT_TICKS: u64 = 60;

pub fn execute_trigger(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let tick = d.payload.get("tick").and_then(|v| v.as_u64()).unwrap_or(0);
    check_llm_timeout(ctx, tick);
    let Some(observed) = ctx.last_observed.clone() else {
        return Ok(LoopStageResult::Noop);
    };
    handle_observed(ctx, &observed)
}

pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != c.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }

    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, true)?;

    let actions = match &c.result {
        CapabilityResult::Llm(llm) => parse_llm_actions(&llm.response),
        _ => Vec::new(),
    };
    if actions.is_empty() {
        return emit_plan(ctx, LoopPlanned {
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
    }

    let req_id = pending.request_id.clone();
    let mut out = Vec::new();
    for action in actions {
        let plan_step_id = Uuid::new_v4().to_string();
        let action_id = plan_step_id.clone();
        let planned_span_id = Uuid::new_v4().to_string();
        match action {
            LlmAction::Patch { path, old, new } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "patch_file".to_string(),
                action_payload: serde_json::json!({ "path": path, "old": old, "new": new }),
                reason: "llm_patch".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
            }),
            LlmAction::Command { cmd, cwd } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "run_command".to_string(),
                action_payload: action_payload_with_cwd(cmd, cwd),
                reason: "llm_command".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
            }),
            LlmAction::Write { path, content } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "write_file".to_string(),
                action_payload: serde_json::json!({ "path": path, "content": content }),
                reason: "llm_write".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
            }),
            LlmAction::Done { reason } => {
                if let Some(goal_text) = &pending.goal_text {
                    let required_loc = extract_required_loc(goal_text);
                    let satisfied = required_loc == 0 || count_loc_in_workspace(&ctx.workspace) >= required_loc;
                    if satisfied {
                        ctx.last_done_goal = pending.goal_text.clone();
                    }
                } else {
                    ctx.last_done_goal = pending.goal_text.clone();
                }
                out.push(LoopPlanned {
                    tick: pending.tick,
                    action_kind: "done".to_string(),
                    action_payload: serde_json::json!({}),
                    reason,
                    llm_request_id: Some(req_id.clone()),
                    trace_id: Some(pending.trace_id.clone()),
                    execution_id: Some(pending.execution_id.clone()),
                    span_id: Some(planned_span_id.clone()),
                    parent_span_id: Some(pending.span_id.clone()),
                    plan_id: Some(pending.plan_id.clone()),
                    plan_step_id: Some(plan_step_id.clone()),
                    action_id: Some(action_id.clone()),
                });
            }
        }
    }
    if out.is_empty() {
        return Ok(LoopStageResult::Noop);
    }
    Ok(LoopStageResult::EmitMany(out.into_iter().map(CanonEvent::LoopPlanned).collect()))
}

pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != f.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, false)?;
    emit_plan(ctx, LoopPlanned {
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
    })
}

fn handle_observed(ctx: &mut LoopContext, observed: &LoopObserved) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_plan.is_some() || ctx.last_planned_observed_tick == Some(observed.tick) {
        return Ok(LoopStageResult::Noop);
    }
    if observed.goal_text.is_none() && observed.error_count == 0 {
        return emit_plan(ctx, LoopPlanned {
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
    }
    if observed.error_count == 0 && ctx.last_done_goal.is_some() && ctx.last_done_goal == observed.goal_text {
        if requirements_satisfied(ctx, observed) {
            return emit_plan(ctx, LoopPlanned {
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
        }
        ctx.last_done_goal = None;
    }

    let request_id = Uuid::new_v4().to_string();
    let trace_id = Uuid::new_v4().to_string();
    let execution_id = Uuid::new_v4().to_string();
    let span_id = Uuid::new_v4().to_string();
    let plan_id = Uuid::new_v4().to_string();
    let include_full_goal = observed.goal_text != ctx.last_prompted_goal;
    let prompt = build_prompt(observed, &ctx.batch_acted, &ctx.batch_tool_results, include_full_goal, &ctx.workspace);
    let llm_call = LlmCall { request_id: request_id.clone(), prompt: prompt.to_string(), role: Some("planner".to_string()) };

    let plan_tool_call_id = Uuid::new_v4().to_string();
    ctx.batch_acted.clear();
    ctx.batch_tool_results.clear();
    ctx.pending_plan = Some(PendingPlan {
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
        ctx.last_prompted_goal = observed.goal_text.clone();
    }
    ctx.last_planned_observed_tick = Some(observed.tick);

    if let Some(emitter) = ctx.emitter.as_ref() {
        canon_meta::canon_emit_meta!(emitter; ToolCall(ToolCall {
            node_id: "plan_consumer".to_string(),
            tool_call_id: plan_tool_call_id,
            request_id: request_id.clone(),
            kind: "llm.plan".to_string(),
            payload: serde_json::json!({"role": "planner"}),
        }));
        canon_meta::canon_emit_meta!(emitter; Llm(LlmCall {
            request_id,
            prompt: llm_call.prompt,
            role: llm_call.role,
        }));
    }

    Ok(LoopStageResult::Deferred)
}

fn requirements_satisfied(ctx: &LoopContext, observed: &LoopObserved) -> bool {
    let Some(goal_text) = observed.goal_text.as_ref() else {
        return false;
    };
    let required_loc = extract_required_loc(goal_text);
    if required_loc == 0 {
        return true;
    }
    let actual_loc = count_loc_in_workspace(&ctx.workspace);
    actual_loc >= required_loc
}

fn check_llm_timeout(ctx: &mut LoopContext, current_tick: u64) {
    let Some(pending) = &ctx.pending_plan else {
        return;
    };
    if current_tick.saturating_sub(pending.dispatched_at_tick) < LLM_TIMEOUT_TICKS {
        return;
    }
    let tick = pending.tick;
    ctx.pending_plan = None;
    let _ = emit_plan(ctx, LoopPlanned {
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

fn emit_plan(_ctx: &LoopContext, payload: LoopPlanned) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Emit(CanonEvent::LoopPlanned(payload)))
}

fn emit_tool_result(ctx: &LoopContext, tool_call_id: &str, request_id: &str, success: bool) -> anyhow::Result<()> {
    if let Some(emitter) = ctx.emitter.as_ref() {
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
    Ok(())
}

enum LlmAction {
    Patch { path: String, old: String, new: String },
    Write { path: String, content: String },
    Command { cmd: String, cwd: Option<String> },
    Done { reason: String },
}

fn build_prompt(observed: &LoopObserved, batch_acted: &[LoopActed], batch_tool_results: &[ToolResult], include_full_goal: bool, workspace: &Path) -> serde_json::Value {
    let _last_action_results_payload: Vec<serde_json::Value> = batch_acted
        .iter()
        .map(|a| serde_json::json!({
            "action_kind": a.action_kind,
            "success": a.success,
            "exit_code": a.exit_code,
            "capability_request_id": a.capability_request_id,
            "tool_call_id": a.tool_call_id,
            "tool_result_id": a.tool_result_id,
            "stdout": a.stdout,
            "stderr": a.stderr,
        }))
        .collect();
    let _last_tool_results_payload: Vec<serde_json::Value> = batch_tool_results
        .iter()
        .map(|r| serde_json::json!({
            "node_id": r.node_id,
            "tool_call_id": r.tool_call_id,
            "tool_result_id": r.tool_result_id,
            "request_id": r.request_id,
            "kind": r.kind,
            "output": r.output,
            "success": r.success,
        }))
        .collect();
    let mut payload = serde_json::json!({
        "goal": observed.goal_text,
        "errors": observed.error_count,
        "warnings": observed.warning_count,
        "workspace_loc": count_loc_in_workspace(workspace),
    });
    if include_full_goal {
        payload["full_goal"] = serde_json::json!(observed.goal_text);
    }
    payload
}

/// Extract fenced json blocks and parse actions.
fn parse_llm_actions(result: &serde_json::Value) -> Vec<LlmAction> {
    let value = result.clone();
    let Some(text) = value.get("text").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let blocks = extract_fenced_blocks(text);
    let mut actions = Vec::new();
    for block in blocks {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&block) {
            if let Some(action) = parse_value_to_action(parsed) {
                actions.push(action);
            }
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&block) {
            for value in parsed {
                if let Some(action) = parse_value_to_action(value) {
                    actions.push(action);
                }
            }
        }
    }
    actions
}

fn extract_required_loc(goal_text: &str) -> usize {
    goal_text
        .lines()
        .filter(|l| l.to_lowercase().contains("loc"))
        .find_map(|l| {
            let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        })
        .unwrap_or(0)
}

fn count_loc_in_workspace(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
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

fn parse_value_to_action(value: serde_json::Value) -> Option<LlmAction> {
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

fn action_payload_with_cwd(cmd: String, cwd: Option<String>) -> serde_json::Value {
    let cwd = cwd.unwrap_or_else(|| ".".to_string());
    serde_json::json!({ "cmd": cmd, "cwd": cwd })
}
