use std::path::Path;

use canon_event::{new_error_occurred, CapabilityCompleted, CapabilityFailed, CapabilityResult, EventId, LlmCall, LoopActed, LoopObserved, LoopPlanned, PlanningCompleted, RouteSelected, RuntimeEvent, ToolCall, ToolResult};
use canon_goal::parse_agent_goal_markdown;
use canon_invariant::decision_trace_payload;
use canon_semantic_state::{derive_self_development_objective_state, LlmSemanticContext, ObjectiveTrendState, SemanticStateSummary};
use canon_tools_search::search_files;
use canon_tools_patch::parse_patch;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

use crate::{
    context::{LoopContext, PendingPlan},
    planning_preconditions,
    policy::{planner_hint_lines, retry_policy_for_planning_context, RetryPolicy},
    result::LoopStageResult,
};

const LLM_TIMEOUT_TICKS: u64 = 60;
const PLACEHOLDER_GOAL: &str = "goal-pending";

pub fn execute_trigger(rs: RouteSelected, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let tick = rs.tick;
    if let Some(timeout_plan) = check_llm_timeout(ctx, tick) {
        if let Some(emitter) = ctx.emitter.as_ref() {
            emitter.emit_with_parents(RuntimeEvent::PlanningCompleted(timeout_plan), vec![trigger_id.clone()], file!(), line!());
        }
    }
    let Some(observed) = ctx.last_observed.clone() else {
        return Ok(LoopStageResult::EmitMany(vec![
            RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "plan_stage".to_string(),
                kind: "plan_suppressed".to_string(),
                payload: decision_trace_payload(
                    "planning skipped because no observation context is available",
                    serde_json::json!({
                        "reason": "missing_last_observed",
                        "tick": tick,
                        "goal_present": ctx.goal_text.is_some(),
                        "consecutive_invalid_plan_batches": ctx.consecutive_invalid_plan_batches,
                    }),
                ),
            }),
            RuntimeEvent::ErrorOccurred(new_error_occurred(
                "plan_stall",
                "plan_stage",
                "planning requested without last_observed context".to_string(),
                "warning",
                serde_json::json!({
                    "reason": "missing_last_observed",
                    "recoverable": true,
                    "tick": tick,
                }),
                None,
            )),
            RuntimeEvent::PlanningCompleted(PlanningCompleted {
                tick,
                llm_request_id: None,
                planned_count: 0,
                status: "missing_observed_context".to_string(),
            }),
        ]));
    };
    handle_observed(ctx, &observed, trigger_id, Some(rs.rationale.clone()), rs.confidence)
}

fn is_placeholder_goal(goal: &str) -> bool {
    let trimmed = goal.trim();
    trimmed.is_empty() || trimmed.contains(PLACEHOLDER_GOAL)
}

pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != c.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }

    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, true, &trigger_id)?;

    let (mut actions, signals) = match &c.result {
        CapabilityResult::Llm(llm) => parse_llm_actions(&llm.response),
        _ => (Vec::new(), None::<serde_json::Value>),
    };

    // If the observed goal is placeholder/empty, ignore any actions (especially "done")
    // and allow routing to re-trigger goal acquisition on next tick.
    let goal_placeholder = pending.goal_text.as_ref().map(|g| is_placeholder_goal(g)).unwrap_or(true);
    if goal_placeholder {
        ctx.last_planned_observed_tick = None;
        return Ok(LoopStageResult::Emit(RuntimeEvent::PlanningCompleted(PlanningCompleted {
            tick: pending.tick,
            llm_request_id: Some(pending.request_id.clone()),
            planned_count: 0,
            status: "goal_placeholder".to_string(),
        })));
    }
    ctx.last_llm_signals = signals.clone();
    if actions.len() > 1 && actions.iter().any(|a| matches!(a.action, LlmAction::Done { .. })) {
        actions.retain(|a| !matches!(a.action, LlmAction::Done { .. }));
    }
    if actions.is_empty() {
        // Parsing failed; clear the planned tick so the next route can re-issue a plan.
        ctx.last_planned_observed_tick = None;
        return Ok(LoopStageResult::Noop);
    }

    let req_id = pending.request_id.clone();
    let mut out = Vec::new();
    for action in actions {
        let plan_step_id = Uuid::new_v4().to_string();
        let action_id = plan_step_id.clone();
        let planned_span_id = Uuid::new_v4().to_string();
        match action.action {
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
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ApplyPatch { patch } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "apply_patch".to_string(),
                action_payload: serde_json::json!({ "patch": patch }),
                reason: "llm_apply_patch".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::Command { cmd, cwd } => {
                let cwd_raw = cwd.as_deref();
                let cwd_default = pending
                    .goal_text
                    .as_deref()
                    .and_then(|t| canon_goal::parse_agent_goal_markdown(t).target_path)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| ctx.workspace.display().to_string());
                let resolved_cwd = cwd_raw.unwrap_or(cwd_default.as_str());
                out.push(LoopPlanned {
                    tick: pending.tick,
                    action_kind: "run_command".to_string(),
                    action_payload: action_payload_with_cwd(cmd, Some(resolved_cwd.to_string())),
                    reason: "llm_command".to_string(),
                    llm_request_id: Some(req_id.clone()),
                    trace_id: Some(pending.trace_id.clone()),
                    execution_id: Some(pending.execution_id.clone()),
                    span_id: Some(planned_span_id.clone()),
                    parent_span_id: Some(pending.span_id.clone()),
                    plan_id: Some(pending.plan_id.clone()),
                    plan_step_id: Some(plan_step_id.clone()),
                    action_id: Some(action_id.clone()),
                    signals: signals.clone(),
                    depends_on: action.depends_on.clone(),
                })
            }
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
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ReadFile { path } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "read_file".to_string(),
                action_payload: serde_json::json!({ "path": path }),
                reason: "llm_read_file".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ListDir { path } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "list_dir".to_string(),
                action_payload: serde_json::json!({ "path": path }),
                reason: "llm_list_dir".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
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
                    signals: signals.clone(),
                    depends_on: action.depends_on.clone(),
                });
            }
        }
    }
    if out.is_empty() {
        return Ok(LoopStageResult::Noop);
    }
    let retry_policy = retry_policy_for_planning_context(
        ctx.last_invalid_plan_reason.as_deref(),
        ctx.consecutive_invalid_plan_batches,
        &ctx.recent_execution_results,
        &ctx.objective_trend_state,
    );
    let semantic_summary = match planning_semantic_summary(ctx.last_observed.as_ref()) {
        Ok(summary) => summary,
        Err(message) => {
            ctx.last_planned_observed_tick = None;
            return Ok(LoopStageResult::EmitMany(vec![
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "plan_stage".to_string(),
                    kind: "plan_suppressed".to_string(),
                    payload: decision_trace_payload(
                        "planning rejected because semantic observation is unavailable",
                        serde_json::json!({
                            "reason": message,
                            "recoverable": true,
                        }),
                    ),
                }),
                RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "plan_stall",
                    "plan_stage",
                    format!("planning requires complete semantic observation: {message}"),
                    "warning",
                    serde_json::json!({
                        "reason": message,
                        "recoverable": true,
                    }),
                    None,
                )),
                RuntimeEvent::PlanningCompleted(PlanningCompleted {
                    tick: pending.tick,
                    llm_request_id: Some(req_id),
                    planned_count: 0,
                    status: "missing_semantic_context".to_string(),
                }),
            ]));
        }
    };
    if let Err(message) = validate_action_batch(
        &out,
        retry_policy,
        &semantic_summary,
        &ctx.objective_trend_state,
        &ctx.recent_execution_results,
    ) {
        ctx.last_planned_observed_tick = None;
        return Ok(LoopStageResult::EmitMany(vec![
            RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "plan_stage".to_string(),
                kind: "invalid_plan_batch".to_string(),
                payload: decision_trace_payload(
                    "plan rejected before execution",
                    serde_json::json!({
                        "reason": message,
                        "planned_count": out.len(),
                        "retry_policy": retry_policy.as_str(),
                    }),
                ),
            }),
            RuntimeEvent::ErrorOccurred(new_error_occurred(
                "invalid_plan_batch",
                "plan_stage",
                format!("invalid plan batch before execution: {message}"),
                "warning",
                serde_json::json!({
                    "planned_count": out.len(),
                    "recoverable": true,
                    "retry_policy": retry_policy.as_str(),
                }),
                None,
            )),
            RuntimeEvent::PlanningCompleted(PlanningCompleted {
                tick: pending.tick,
                llm_request_id: Some(req_id),
                planned_count: 0,
                status: "invalid_plan".to_string(),
            }),
        ]));
    }
    // Action-batch dedup: if LLM returned identical actions to the previous plan, skip.
    let action_batch_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for p in &out {
            p.action_kind.hash(&mut h);
            p.action_payload.to_string().hash(&mut h);
        }
        h.finish()
    };
    if ctx.last_emitted_plan_hash == Some(action_batch_hash) {
        ctx.last_planned_observed_tick = None;
        return Ok(LoopStageResult::Noop);
    }
    ctx.last_emitted_plan_hash = Some(action_batch_hash);
    let mut events: Vec<RuntimeEvent> = out.into_iter().map(RuntimeEvent::LoopPlanned).collect();
    events.push(RuntimeEvent::PlanningCompleted(PlanningCompleted {
        tick: pending.tick,
        llm_request_id: Some(req_id),
        planned_count: events.len(),
        status: "planned".to_string(),
    }));
    Ok(LoopStageResult::EmitMany(events))
}

fn validate_action_batch(
    actions: &[LoopPlanned],
    retry_policy: RetryPolicy,
    semantic_summary: &SemanticStateSummary,
    objective_trend_state: &ObjectiveTrendState,
    recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord],
) -> Result<(), String> {
    if !semantic_summary.complete {
        return Err("semantic summary is incomplete".to_string());
    }
    let target_root = semantic_summary
        .target_root
        .as_ref()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "semantic summary is missing target_root".to_string())?;
    let preconditions =
        planning_preconditions::derive_preconditions_from_lines(&semantic_summary.planning_preconditions);
    let has_discovery = actions.iter().any(|a| matches!(a.action_kind.as_str(), "list_dir" | "read_file"));
    let has_execution = actions.iter().any(|a| {
        matches!(
            a.action_kind.as_str(),
            "patch_file" | "apply_patch" | "write_file" | "run_command" | "done"
        )
    });
    if retry_policy == RetryPolicy::DiscoveryOnly && has_execution {
        return Err(
            "discovery-only retry required after invalid plan batch; execution/edit actions are not allowed yet"
                .to_string(),
        );
    }
    if retry_policy == RetryPolicy::SinglePatchOnly {
        let apply_patch_count = actions.iter().filter(|a| a.action_kind == "apply_patch").count();
        let has_non_patch = actions.iter().any(|a| a.action_kind != "apply_patch");
        if apply_patch_count != 1 || has_non_patch {
            return Err(
                "single-patch retry required after apply_patch failure; emit exactly one apply_patch action and nothing else"
                    .to_string(),
            );
        }
    }
    if has_discovery && has_execution {
        return Err(
            "mixed discovery actions with execution/edit actions in one plan batch".to_string(),
        );
    }

    for action in actions {
        match action.action_kind.as_str() {
            "apply_patch" => {
                let Some(patch) = action.action_payload.get("patch").and_then(|v| v.as_str()) else {
                    return Err("apply_patch missing string patch payload".to_string());
                };
                if let Err(err) = parse_patch(patch) {
                    return Err(format!("apply_patch payload is invalid: {err}"));
                }
            }
            "run_command" => {
                let cwd = action.action_payload.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
                if cwd.is_empty() || !Path::new(cwd).is_absolute() {
                    return Err(format!(
                        "run_command requires an absolute cwd; got {:?}",
                        if cwd.is_empty() { "<empty>" } else { cwd }
                    ));
                }
            }
            "read_file" | "list_dir" | "write_file" | "patch_file" => {
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err(format!("{} missing path payload", action.action_kind));
                };
                validate_workspace_relative_path(path, &target_root)
                    .map_err(|e| format!("{} path is invalid: {e}", action.action_kind))?;
            }
            "done" => {}
            other => {
                return Err(format!("unknown plan action_kind {other}"));
            }
        }
    }

    planning_preconditions::validate_preconditions(
        actions,
        &target_root,
        &preconditions,
        semantic_summary,
    )?;
    planning_preconditions::validate_objective_route_plan_alignment(
        actions,
        &target_root,
        "plan",
        objective_trend_state.primary_objective(
            &canon_semantic_state::derive_self_development_objective_state(
                semantic_summary,
                0,
                &[],
            ),
        ),
        semantic_summary,
    )?;
    planning_preconditions::validate_trend_intent_alignment(
        actions,
        &target_root,
        recent_execution_results,
        objective_trend_state,
    )?;

    Ok(())
}

fn planning_semantic_summary(observed: Option<&LoopObserved>) -> Result<SemanticStateSummary, String> {
    let observed = observed.ok_or_else(|| "last_observed is missing".to_string())?;
    let summary = observed.semantic_summary.clone();
    if !summary.complete {
        return Err("semantic summary is incomplete".to_string());
    }
    if summary.target_root.is_none() {
        return Err("semantic summary is missing target_root".to_string());
    }
    Ok(summary)
}

fn validate_workspace_relative_path(path: &str, _target_root: &Path) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err("absolute paths are not allowed for plan discovery/edit actions".to_string());
    }
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("path escapes target workspace via parent traversal: {path}"));
    }
    Ok(())
}

pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != f.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, false, &trigger_id)?;
    Ok(LoopStageResult::Emit(RuntimeEvent::PlanningCompleted(PlanningCompleted {
        tick: pending.tick,
        llm_request_id: Some(pending.request_id),
        planned_count: 0,
        status: "llm_failed".to_string(),
    })))
}

fn handle_observed(
    ctx: &mut LoopContext,
    observed: &LoopObserved,
    trigger_id: EventId,
    route_rationale: Option<String>,
    route_confidence: Option<f32>,
) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_plan.is_some() || ctx.last_planned_observed_tick == Some(observed.tick) {
        return Ok(LoopStageResult::Noop);
    }
    let semantic_summary = match planning_semantic_summary(Some(observed)) {
        Ok(summary) => summary,
        Err(message) => {
            return Ok(LoopStageResult::EmitMany(vec![
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "plan_stage".to_string(),
                    kind: "plan_suppressed".to_string(),
                    payload: decision_trace_payload(
                        "planning rejected because semantic observation is unavailable",
                        serde_json::json!({
                            "reason": message,
                            "recoverable": true,
                            "tick": observed.tick,
                        }),
                    ),
                }),
                RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "plan_stall",
                    "plan_stage",
                    format!("planning requires complete semantic observation: {message}"),
                    "warning",
                    serde_json::json!({
                        "reason": message,
                        "recoverable": true,
                        "tick": observed.tick,
                    }),
                    None,
                )),
                RuntimeEvent::PlanningCompleted(PlanningCompleted {
                    tick: observed.tick,
                    llm_request_id: None,
                    planned_count: 0,
                    status: "missing_semantic_context".to_string(),
                }),
            ]));
        }
    };
    let observed_hash = hash_observed(observed, &semantic_summary);
    if ctx.last_handled_observed_hash == Some(observed_hash) {
        return Ok(LoopStageResult::Noop);
    }
    if observed.goal_text.as_ref().map(|g| is_placeholder_goal(g)).unwrap_or(true) && observed.error_count == 0 {
        // Wait state: goal not yet available. Emit nothing — silence is the correct response.
        // Emitting LoopPlanned{no_op} here only generates log spam: every agent whose
        // RouteExecutor fires idle_plan (triggered by another agent's LoopObserved) would
        // produce a no_op plan, cascading through the bus with zero useful effect.
        return Ok(LoopStageResult::Noop);
    }
    if observed.error_count == 0 && ctx.last_done_goal.is_some() && ctx.last_done_goal == observed.goal_text {
        if requirements_satisfied(ctx, observed) {
            return Ok(LoopStageResult::Emit(RuntimeEvent::PlanningCompleted(PlanningCompleted {
                tick: observed.tick,
                llm_request_id: None,
                planned_count: 0,
                status: "goal_complete".to_string(),
            })));
        }
        ctx.last_done_goal = None;
    }
    ctx.last_handled_observed_hash = Some(observed_hash);

    // --- THREE-TIER PROMPT CACHING ---
    // Tier 1 (system): static instructions — sent once, cached in executor worker.
    // Tier 2 (context_base): GOAL + workspace tree + facts + search hints — sent only on change.
    // Tier 3 (prompt / delta): LOC, errors, recent actions/results — sent every call.
    let sub_agent_section = ctx.context_merger.prompt_section();
    let workspace_clone = ctx.workspace.clone();
    let target_workspace = semantic_summary
        .target_root
        .clone()
        .unwrap_or_else(|| workspace_clone.display().to_string());
    const HEURISTIC_RATIONALE: &str = "heuristic proposal from runtime state";
    let (rationale_for_prompt, confidence_for_prompt) = match (route_rationale.as_deref(), route_confidence) {
        (Some(r), c) if !r.is_empty() && r != HEURISTIC_RATIONALE => (Some(r), c.map(|v| v as f64)),
        _ => (ctx.last_route_rationale_non_empty.as_deref(), ctx.last_route_confidence_non_empty),
    };
    let llm_semantic_context = build_llm_semantic_context(
        &semantic_summary,
        observed,
        &ctx.batch_acted,
        &ctx.batch_tool_results,
        &ctx.recent_execution_results,
        &target_workspace,
        rationale_for_prompt,
        confidence_for_prompt,
        ctx.last_invalid_plan_reason.as_deref(),
        ctx.last_invalid_plan_planned_count,
        ctx.consecutive_invalid_plan_batches,
        &ctx.objective_trend_state,
    );
    let context_base = build_context_base(observed, &workspace_clone, &sub_agent_section, &llm_semantic_context);
    let context_base_hash = hash_str(&context_base);

    let context_delta = build_context_delta(
        &llm_semantic_context,
        &ctx.batch_acted,
        ctx.last_invalid_plan_reason.as_deref(),
        ctx.consecutive_invalid_plan_batches,
    );

    let system_id = *PLANNER_SYSTEM_PROMPT_ID;
    let send_system = ctx.last_system_prompt_id != Some(system_id);
    let send_base = ctx.last_context_base_id != Some(context_base_hash);

    // Drop: nothing changed at any tier.
    let delta_hash = hash_str(&context_delta);
    if !send_system && !send_base && ctx.last_delta_hash == Some(delta_hash) {
        return Ok(LoopStageResult::Noop);
    }

    // Update tracking state.
    let prev_prompt_id = ctx.last_system_prompt_id.map(|id| id.to_string());
    ctx.last_system_prompt_id = Some(system_id);
    if send_base {
        ctx.last_context_base_id = Some(context_base_hash);
    }
    ctx.last_delta_hash = Some(delta_hash);

    let request_id = Uuid::new_v4().to_string();
    let trace_id = Uuid::new_v4().to_string();
    let execution_id = Uuid::new_v4().to_string();
    let span_id = Uuid::new_v4().to_string();
    let plan_id = Uuid::new_v4().to_string();

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
    ctx.last_prompted_goal = observed.goal_text.clone();
    ctx.last_planned_observed_tick = Some(observed.tick);

    if let Some(emitter) = ctx.emitter.as_ref() {
        emitter.emit_with_parents(RuntimeEvent::ToolCall(ToolCall {
            node_id: "plan_consumer".to_string(),
            tool_call_id: plan_tool_call_id,
            request_id: request_id.clone(),
            kind: "llm.plan".to_string(),
            payload: serde_json::json!({"role": "planner"}),
            accepted: true,
        }), vec![trigger_id.clone()], file!(), line!());
        emitter.emit_with_parents(RuntimeEvent::Llm(LlmCall {
            request_id,
            // Fast-changing delta only — GOAL and workspace live in context_base / executor cache.
            prompt: context_delta,
            role: Some("planner".to_string()),
            agent_id: ctx.agent_id.clone(),
            dispatched: true,
            // Tier 1: static system instructions — sent only on first call or session reset.
            system: send_system.then(|| PLANNER_SYSTEM_INSTRUCTIONS.to_string()),
            system_prompt_id: Some(system_id.to_string()),
            // Tier 2: slow-changing context base — sent only when changed.
            context_base: send_base.then_some(context_base),
            context_base_id: Some(context_base_hash.to_string()),
            prompt_base_id: Some(system_id.to_string()),
            prev_prompt_id,
        }), vec![trigger_id.clone()], file!(), line!());
    }

    Ok(LoopStageResult::Deferred)
}

fn hash_observed(observed: &LoopObserved, semantic_summary: &SemanticStateSummary) -> u64 {
    let mut h = DefaultHasher::new();
    observed.error_count.hash(&mut h);
    // warning_count excluded: watchdog stall warnings fire every tick and would change
    // the hash on every cycle, triggering a new plan LLM call after each completion even
    // when goal, errors, and workspace state are identical.
    observed.goal_text.hash(&mut h);
    semantic_summary.hash(&mut h);
    h.finish()
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

fn check_llm_timeout(ctx: &mut LoopContext, current_tick: u64) -> Option<PlanningCompleted> {
    let Some(pending) = &ctx.pending_plan else {
        return None;
    };
    if current_tick.saturating_sub(pending.dispatched_at_tick) < LLM_TIMEOUT_TICKS {
        return None;
    }
    let tick = pending.tick;
    ctx.pending_plan = None;
    Some(PlanningCompleted {
        tick,
        llm_request_id: None,
        planned_count: 0,
        status: "llm_timeout".to_string(),
    })
}

fn emit_tool_result(ctx: &LoopContext, tool_call_id: &str, request_id: &str, success: bool, trigger_id: &EventId) -> anyhow::Result<()> {
    if let Some(emitter) = ctx.emitter.as_ref() {
        emitter.emit_with_parents(RuntimeEvent::ToolResult(ToolResult {
            node_id: "plan_consumer".to_string(),
            tool_call_id: tool_call_id.to_string(),
            tool_result_id: Uuid::new_v4().to_string(),
            request_id: request_id.to_string(),
            kind: "llm.plan".to_string(),
            output: serde_json::json!({}),
            success,
        }), vec![trigger_id.clone()], file!(), line!());
    }
    Ok(())
}

#[derive(Clone)]
enum LlmAction {
    Patch { path: String, old: String, new: String },
    Write { path: String, content: String },
    Command { cmd: String, cwd: Option<String> },
    Done { reason: String },
    ApplyPatch { patch: String },
    ReadFile { path: String },
    ListDir { path: String },
}

#[derive(Clone)]
struct ActionPlan {
    action: LlmAction,
    depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// System prompt — static, immutable, sent once per executor session.
// prompt_id = hash(PLANNER_SYSTEM_INSTRUCTIONS); computed once at startup.
// ---------------------------------------------------------------------------

const PLANNER_SYSTEM_INSTRUCTIONS: &str = r#"You are a code-editing agent. Produce a plan as a JSON array of actions.

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list what files/dirs exist (use BEFORE assuming project state)
   {"action":"list_dir","path":"."}

2. read_file — read a file's current contents when you do not already have enough context to edit safely
   {"action":"read_file","path":"src/main.rs"}
   ⚠ Results appear in "Recent actions" on your NEXT call. Do not mix with edits.

3. apply_patch — create, update, or delete files  ← ONLY tool for file edits
   {"action":"apply_patch","patch":"*** Begin Patch\n...\n*** End Patch"}

   Patch format (paths MUST be relative to TARGET WORKSPACE):

   *** Begin Patch
   *** Add File: path/to/new.rs        ← create new file
   +fn hello() {}
   +
   *** Update File: path/to/existing.rs ← edit existing file
   @@ fn main                           ← optional context (function/class name)
    fn main() {                        ←  space = unchanged context line
   -    println!("old");               ← - = remove this line
   +    println!("new");               ← + = add this line
    }
   *** Delete File: path/to/remove.rs  ← delete file
   *** End Patch

   Rules:
   - *** Add File for new files, *** Update File for existing files
   - Include 3 lines of unchanged context around each change
   - Multiple file ops can be in one patch
   - NEVER use absolute paths inside the patch string
   - NEVER prefix paths with the project directory name (use `src/main.rs`, NOT `myproject/src/main.rs`)

4. run_command — run a shell command
   {"action":"run_command","cmd":"cargo build","cwd":"<TARGET_WORKSPACE>"}
   cwd must be absolute. Use TARGET WORKSPACE (provided in context) or a subdir.

5. done — declare goal complete
   {"action":"done","reason":"..."}

━━━ WORKFLOW ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Step 1 — Discover (only when unsure of project state or missing file contents):
  Emit ONLY list_dir and/or read_file. Do NOT mix with edits.
  → Results appear in "Recent actions" on your next call.

Step 2 — Create/Edit (after seeing discovery results):
  Use apply_patch (*** Add File for new, *** Update File for existing).
  Use run_command for cargo/shell operations.
  The "done" action must be the ONLY action in a batch, and only after verification has shown the goal is met.

NEVER use "write" or "patch_file" — they are removed. Use apply_patch.
NEVER assume a directory/project exists without checking with list_dir first.
WORKSPACE RULE: If the target project directory already exists in the workspace tree, use `cargo init --name <name>` instead of `cargo new`. `cargo new` fails when the directory exists.
SAFETY RULE: The following commands are BLOCKED and will always fail. Do NOT plan them: rm -rf, git reset --hard, git clean -f, dd if=, mkfs, shred, >/dev/sd.

━━━ OUTPUT FORMAT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Return ONLY a JSON array of action objects. Do NOT wrap in an object. Do NOT include a "signals" key.
Example:
[
  {"action":"list_dir","path":"."},
  {"action":"run_command","cmd":"cargo build","cwd":"<TARGET_WORKSPACE>"}
]

Rules:
- If you believe the goal is complete, the array must contain exactly one item: {"action":"done","reason":"..."}
- Never include "done" alongside any other action.
- Optional on any action: `"depends_on": ["<action_id>"]` to defer dispatch until those actions succeed.
No prose outside the code block."#;

/// Computed once at startup from the hash of the static system instructions.
/// Used as the cache key sent in every `LlmCall.system_prompt_id`.
static PLANNER_SYSTEM_PROMPT_ID: std::sync::LazyLock<u64> =
    std::sync::LazyLock::new(|| hash_str(PLANNER_SYSTEM_INSTRUCTIONS));

/// Tier-2 context: slow-changing section containing GOAL and workspace state.
/// Sent only when its hash differs from `ctx.last_context_base_id`. For stateful
/// endpoints the LLM already has this in session history; for stateless endpoints
/// the executor worker reconstructs it from its cache before each API call.
fn build_context_base(
    observed: &LoopObserved,
    workspace: &Path,
    sub_agent_section: &str,
    llm_semantic_context: &LlmSemanticContext,
) -> String {
    let goal_text = observed.goal_text.clone().unwrap_or_else(|| "<no goal provided>".to_string());
    let target_workspace = llm_semantic_context
        .target_workspace
        .clone()
        .or_else(|| llm_semantic_context.semantic_summary.target_root.clone())
        .unwrap_or_else(|| workspace.display().to_string());
    let semantic_planner_block = llm_semantic_context.render_planner_base_block();

    let search_hints = build_search_hints(&goal_text, workspace);
    let workspace_tree = build_workspace_tree(std::path::Path::new(&target_workspace), 3, 0);

    format!(
        r#"GOAL:
{goal_text}

## Workspace State
{workspace_tree}

{semantic_planner_block}

━━━ CONTEXT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Relevant files:{search_hints}

{sub_agent_section}"#,
        goal_text = goal_text,
        semantic_planner_block = semantic_planner_block,
        workspace_tree = workspace_tree,
        search_hints = search_hints,
        sub_agent_section = sub_agent_section,
    )
}

/// Tier-3 context: fast-changing delta sent on every planning call.
/// Contains only the fields that change after each action: LOC, error counts,
/// recent actions and tool results. Does NOT include GOAL or workspace tree.
fn build_context_delta(
    llm_semantic_context: &LlmSemanticContext,
    batch_acted: &[LoopActed],
    last_invalid_plan_reason: Option<&str>,
    consecutive_invalid_plan_batches: u32,
) -> String {
    let destructive_warning = batch_acted.iter().any(|a| a.stderr.trim() == "rejected_destructive_command");
    let destructive_note = if destructive_warning {
        "WARNING: A previous plan was blocked as destructive. Do NOT include destructive commands; they will fail.\n"
    } else {
        ""
    };

    let invalid_plan_section = match last_invalid_plan_reason {
        Some(reason) => {
            let policy = retry_policy_for_planning_context(
                Some(reason),
                consecutive_invalid_plan_batches,
                &llm_semantic_context.recent_execution_results,
                &llm_semantic_context.objective_trend_state,
            );
            let policy_text = match policy {
                RetryPolicy::DiscoveryOnly =>
                    "Retry policy: discovery-only. Emit ONLY list_dir/read_file on the next batch.",
                RetryPolicy::SinglePatchOnly =>
                    "Retry policy: single-patch-only. Emit exactly one apply_patch action and nothing else on the next batch.",
                RetryPolicy::CorrectiveRetry =>
                    "Retry policy: corrective retry. Fix the specific invalid-plan issue and retry directly; discovery is not required unless you are missing file context.",
                RetryPolicy::None => "Retry policy: none.",
            };
            format!("{}\n{policy_text}", llm_semantic_context.render_planner_delta_block())
        }
        None => {
            let policy = retry_policy_for_planning_context(
                None,
                consecutive_invalid_plan_batches,
                &llm_semantic_context.recent_execution_results,
                &llm_semantic_context.objective_trend_state,
            );
            if policy == RetryPolicy::CorrectiveRetry {
                format!(
                    "{}\nRetry policy: corrective retry. Recent execution made no semantic progress; change the repair strategy before retrying.",
                    llm_semantic_context.render_planner_delta_block()
                )
            } else {
                llm_semantic_context.render_planner_delta_block()
            }
        }
    };

    let planner_hint = build_planner_hint(
        batch_acted,
        last_invalid_plan_reason,
        consecutive_invalid_plan_batches,
        &llm_semantic_context.recent_execution_results,
        &llm_semantic_context.objective_trend_state,
    );

    format!(
        r#"{invalid_plan_section}

Planner hint:
{planner_hint}

{destructive_note}"#,
        invalid_plan_section = invalid_plan_section,
        planner_hint = planner_hint,
        destructive_note = destructive_note,
    )
}

fn build_llm_semantic_context(
    semantic_summary: &SemanticStateSummary,
    observed: &LoopObserved,
    batch_acted: &[LoopActed],
    batch_tool_results: &[ToolResult],
    recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord],
    target_workspace: &str,
    route_rationale: Option<&str>,
    route_confidence: Option<f64>,
    last_invalid_plan_reason: Option<&str>,
    last_invalid_plan_planned_count: Option<usize>,
    consecutive_invalid_plan_batches: u32,
    objective_trend_state: &ObjectiveTrendState,
) -> LlmSemanticContext {
    let recent_actions = batch_acted
        .iter()
        .rev()
        .take(24)
        .map(|action| {
            let mut entry =
                format!("- action={} success={} exit_code={:?}", action.action_kind, action.success, action.exit_code);
            let stdout = action.stdout.trim();
            let stderr = action.stderr.trim();
            if !stdout.is_empty() {
                let truncated = if stdout.len() > 800 { &stdout[..800] } else { stdout };
                entry.push_str(&format!("\n  stdout: {truncated}"));
            }
            if !stderr.is_empty() {
                let truncated = if stderr.len() > 400 { &stderr[..400] } else { stderr };
                entry.push_str(&format!("\n  stderr: {truncated}"));
            }
            entry
        })
        .collect::<Vec<_>>();
    let recent_tool_results = batch_tool_results
        .iter()
        .rev()
        .take(12)
        .map(|result| {
            let content =
                serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| result.output.to_string());
            let truncated = if content.len() > 600 { &content[..600] } else { &content };
            format!("- kind={} success={}\n  output: {}", result.kind, result.success, truncated)
        })
        .collect::<Vec<_>>();
    LlmSemanticContext {
        mission_summary: observed
            .goal_text
            .as_deref()
            .map(parse_agent_goal_markdown)
            .map(|goal| canon_goal::summarize_goal(&goal)),
        semantic_summary: semantic_summary.clone(),
        objective_state: derive_self_development_objective_state(
            semantic_summary,
            consecutive_invalid_plan_batches,
            recent_execution_results,
        ),
        objective_trend_state: objective_trend_state.clone(),
        target_workspace: Some(target_workspace.to_string()),
        workspace_loc: Some(count_loc_in_workspace(std::path::Path::new(target_workspace))),
        error_count: Some(observed.error_count),
        warning_count: Some(observed.warning_count),
        route_rationale: route_rationale.map(str::to_string),
        route_confidence,
        invalid_plan_reason: last_invalid_plan_reason.map(str::to_string),
        invalid_plan_planned_count: last_invalid_plan_planned_count,
        consecutive_invalid_plan_batches,
        low_level_diagnostics: observed.observe_diagnostics.clone(),
        recent_actions,
        recent_tool_results,
        recent_execution_results: recent_execution_results.to_vec(),
    }
}

fn build_planner_hint(
    batch_acted: &[LoopActed],
    last_invalid_plan_reason: Option<&str>,
    consecutive_invalid_plan_batches: u32,
    recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord],
    objective_trend_state: &ObjectiveTrendState,
) -> String {
    let last_failure = if recent_execution_results.is_empty() {
        batch_acted
            .iter()
            .rev()
            .find(|a| !a.success && (!a.stderr.trim().is_empty() || !a.stdout.trim().is_empty()))
            .map(|a| {
                (
                    a.action_kind.clone(),
                    if !a.stderr.trim().is_empty() {
                        a.stderr.clone()
                    } else {
                        a.stdout.clone()
                    },
                )
            })
    } else {
        None
    };
    let hint_lines = planner_hint_lines(
        last_invalid_plan_reason,
        consecutive_invalid_plan_batches,
        recent_execution_results,
        objective_trend_state,
        last_failure.as_ref().map(|(kind, _)| kind.as_str()),
        last_failure
            .as_ref()
            .map(|(_, text)| truncate_hint_text(text, 240))
            .as_deref(),
    );
    if hint_lines.is_empty() {
        "none".to_string()
    } else {
        hint_lines.join("\n")
    }
}

fn truncate_hint_text(text: &str, max_len: usize) -> String {
    let trimmed = text.trim().replace('\n', " ");
    if trimmed.len() > max_len {
        format!("{}...", &trimmed[..max_len])
    } else {
        trimmed
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn build_search_hints(goal_text: &str, workspace: &Path) -> String {
    let spec = parse_agent_goal_markdown(goal_text);
    let target_root = spec.target_path.clone().map(|p| workspace.join(p)).unwrap_or_else(|| workspace.to_path_buf());
    if !target_root.exists() {
        return " (none)".to_string();
    }

    let keywords = extract_goal_keywords(&spec);
    if keywords.is_empty() {
        return " (none)".to_string();
    }

    let mut lines = Vec::new();
    for kw in keywords.into_iter().take(3) {
        if let Ok(results) = search_files(&kw, &target_root, 5) {
            for r in results {
                lines.push(format!("\n- {kw}: {}", r.path.display()));
            }
        }
    }
    if lines.is_empty() {
        " (none)".to_string()
    } else {
        lines.join("")
    }
}

fn extract_goal_keywords(spec: &canon_goal::GoalSpec) -> Vec<String> {
    let mut out = Vec::new();
    for req in &spec.requirements {
        for token in req.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '/') {
            if token.len() >= 4 || token.contains('.') || token.contains('/') {
                out.push(token.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Parse LLM response into actions and an optional signals object.
/// Supports three response shapes:
///   A) {"signals":{...},"actions":[...]}  — wrapper format (preferred)
///   B) bare JSON array                    — legacy / fence-stripped
///   C) {"text":"```json\n...\n```"}       — text wrapper with fenced blocks
fn parse_llm_actions(result: &serde_json::Value) -> (Vec<ActionPlan>, Option<serde_json::Value>) {
    // Shape A: wrapper object with "actions" key
    if result.is_object() && result.get("actions").is_some() {
        let signals = result.get("signals").cloned();
        let actions = result["actions"].as_array().map(|arr| arr.iter().filter_map(|v| parse_value_to_action(v.clone())).collect()).unwrap_or_default();
        return (actions, signals);
    }

    // Shape B: bare JSON array
    if let Some(arr) = result.as_array() {
        let actions = arr.iter().filter_map(|v| parse_value_to_action(v.clone())).collect();
        return (actions, None);
    }

    // Shape B: single action object
    if result.is_object() && result.get("action").is_some() {
        if let Some(action) = parse_value_to_action(result.clone()) {
            return (vec![action], None);
        }
    }

    // Shape C: {"text":"```json\n...\n```"} — extract fenced blocks
    let Some(text) = result.get("text").and_then(|v| v.as_str()) else {
        return (Vec::new(), None);
    };
    let blocks = extract_fenced_blocks(text);
    let mut actions = Vec::new();
    let mut signals: Option<serde_json::Value> = None;
    for block in blocks {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&block) {
            // Check for wrapper shape inside fenced block
            if parsed.is_object() && parsed.get("actions").is_some() {
                signals = parsed.get("signals").cloned();
                if let Some(arr) = parsed["actions"].as_array() {
                    for v in arr {
                        if let Some(a) = parse_value_to_action(v.clone()) {
                            actions.push(a);
                        }
                    }
                }
                continue;
            }
            if let Some(arr) = parsed.as_array() {
                for v in arr {
                    if let Some(a) = parse_value_to_action(v.clone()) {
                        actions.push(a);
                    }
                }
            } else if let Some(a) = parse_value_to_action(parsed) {
                actions.push(a);
            }
        }
    }
    (actions, signals)
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
            // Skip build artifact and hidden directories.
            let skip = path.file_name().and_then(|n| n.to_str()).map(|n| {
                matches!(n, "target" | ".git" | "node_modules" | ".cargo")
            }).unwrap_or(false);
            if !skip {
                total += count_loc_in_workspace(&path);
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}

/// Build a compact directory tree (depth-limited, capped at 40 lines).
/// Skips hidden dirs (`.git`, `target`, `node_modules`).
fn build_workspace_tree(dir: &Path, max_depth: usize, depth: usize) -> String {
    const MAX_LINES: usize = 40;
    let mut lines: Vec<String> = Vec::new();
    build_workspace_tree_inner(dir, depth, max_depth, &mut lines, MAX_LINES);
    if lines.is_empty() {
        "(directory not found or empty)".to_string()
    } else {
        lines.join("\n")
    }
}

fn build_workspace_tree_inner(dir: &Path, depth: usize, max_depth: usize, lines: &mut Vec<String>, cap: usize) {
    if lines.len() >= cap {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let indent = "  ".repeat(depth);
    let mut items: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    items.sort();
    for path in items {
        if lines.len() >= cap {
            break;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip hidden and build dirs
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            lines.push(format!("{indent}{name}/"));
            if depth < max_depth {
                build_workspace_tree_inner(&path, depth + 1, max_depth, lines, cap);
            }
        } else {
            lines.push(format!("{indent}{name}"));
        }
    }
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

fn parse_value_to_action(value: serde_json::Value) -> Option<ActionPlan> {
    // Handle "action" discriminator format (used by planner GPT)
    if let Some(action_str) = value.get("action").and_then(|v| v.as_str()) {
        let depends_on = value.get("depends_on").and_then(|d| d.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        match action_str {
            "done" => {
                let reason = value.get("reason").and_then(|v| v.as_str()).unwrap_or("done").to_string();
                return Some(ActionPlan { action: LlmAction::Done { reason }, depends_on });
            }
            "apply_patch" => {
                let patch = value.get("patch").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::ApplyPatch { patch: patch.to_string() }, depends_on });
            }
            "write" | "write_file" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let content = value.get("content").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::Write { path: path.to_string(), content: content.to_string() }, depends_on });
            }
            "run_command" | "command" => {
                let cmd = value.get("cmd").and_then(|v| v.as_str())?;
                let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                return Some(ActionPlan { action: LlmAction::Command { cmd: cmd.to_string(), cwd }, depends_on });
            }
            "patch_file" | "patch" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let old = value.get("old").and_then(|v| v.as_str())?;
                let new = value.get("new").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::Patch { path: path.to_string(), old: old.to_string(), new: new.to_string() }, depends_on });
            }
            "read_file" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::ReadFile { path: path.to_string() }, depends_on });
            }
            "list_dir" => {
                let path = value.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                return Some(ActionPlan { action: LlmAction::ListDir { path: path.to_string() }, depends_on });
            }
            _ => return None,
        }
    }
    // Fallback: key-based schema (no "action" field)
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        let reason = value.get("reason").and_then(|v| v.as_str()).unwrap_or("done").to_string();
        return Some(ActionPlan { action: LlmAction::Done { reason }, depends_on: Vec::new() });
    }
    if let Some(cmd) = value.get("cmd").and_then(|v| v.as_str()) {
        let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        return Some(ActionPlan { action: LlmAction::Command { cmd: cmd.to_string(), cwd }, depends_on: Vec::new() });
    }
    if let Some(patch) = value.get("patch").and_then(|v| v.as_str()) {
        return Some(ActionPlan { action: LlmAction::ApplyPatch { patch: patch.to_string() }, depends_on: Vec::new() });
    }
    if let (Some(write_path), Some(content)) = (value.get("write").and_then(|v| v.as_str()), value.get("content").and_then(|v| v.as_str())) {
        return Some(ActionPlan { action: LlmAction::Write { path: write_path.to_string(), content: content.to_string() }, depends_on: Vec::new() });
    }
    let path = value.get("path").and_then(|v| v.as_str())?;
    let old = value.get("old").and_then(|v| v.as_str())?;
    let new = value.get("new").and_then(|v| v.as_str())?;
    Some(ActionPlan { action: LlmAction::Patch { path: path.to_string(), old: old.to_string(), new: new.to_string() }, depends_on: Vec::new() })
}

fn action_payload_with_cwd(cmd: String, cwd: Option<String>) -> serde_json::Value {
    let cwd = cwd.unwrap_or_else(|| ".".to_string());
    serde_json::json!({ "cmd": cmd, "cwd": cwd })
}
