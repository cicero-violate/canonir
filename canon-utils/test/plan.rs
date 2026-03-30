use std::path::Path;

use canon_analysis::{graph_backed_module_moves, graph_backed_rename_candidates};
use canon_event::{
    new_error_occurred, CapabilityCompleted, CapabilityFailed, CapabilityResult, EventId, LlmCall, LoopActed, LoopObserved, LoopPlanned, PlanningCompleted, RouteSelected, RuntimeEvent, ToolCall,
    ToolResult,
};
use canon_goal::parse_agent_goal_markdown;
use canon_invariant::{
    decision_trace_payload, drain_persisted_store_events, meta_invariant_action_must_declare_verifier, meta_invariant_all_failures_typed, meta_invariant_any_action_cites_failure,
    meta_invariant_expected_verifier, meta_invariant_has_actionable_failure, meta_invariant_is_mutating_action, observe_failure_fingerprint, ConstraintRoute, ConstraintState, FailureFingerprint,
    PersistedInvariantStoreEventKind,
};
use canon_semantic_state::{
    derive_self_development_objective_state, primary_development_objective_kind, primary_development_strategy_kind, DevelopmentObjectiveKind, DevelopmentStrategyKind, LlmSemanticContext,
    ObjectiveTrendState, SemanticStateSummary,
};
use canon_skills::global_registry;
use canon_tools_patch::parse_patch;
use canon_tools_search::search_files;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use uuid::Uuid;

use crate::{
    context::{LoopContext, PendingPlan},
    env_model::{select_bootstrap_command, BootstrapCommandChoice},
    planning_preconditions,
    policy::{planner_hint_lines, retry_policy_for_planning_context, semantic_planner_hint_lines, RetryPolicy},
    result::LoopStageResult,
};

const LLM_TIMEOUT_TICKS: u64 = 60;
const PLACEHOLDER_GOAL: &str = "goal-pending";

fn retry_policy_text(policy: RetryPolicy, contextualized: bool) -> &'static str {
    match (policy, contextualized) {
        (RetryPolicy::DiscoveryOnly, _) => "Retry policy: discovery-only. Emit ONLY list_dir/read_file on the next batch.",
        (RetryPolicy::SinglePatchOnly, _) => "Retry policy: single-patch-only. Emit exactly one apply_patch action and nothing else on the next batch.",
        (RetryPolicy::CorrectiveRetry, true) => {
            "Retry policy: corrective retry. Fix the specific invalid-plan issue and retry directly; discovery is not required unless you are missing file context."
        }
        (RetryPolicy::CorrectiveRetry, false) => "Retry policy: corrective retry. Change the repair strategy before retrying.",
        (RetryPolicy::None, _) => "Retry policy: none.",
    }
}

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
            RuntimeEvent::PlanningCompleted(PlanningCompleted { tick, llm_request_id: None, planned_count: 0, status: "missing_observed_context".to_string() }),
        ]));
    };
    if let Some(result) = deterministic_bootstrap_plan(&rs, ctx, &observed)? {
        return Ok(result);
    }
    handle_observed(ctx, &observed, trigger_id, Some(rs.rationale.clone()), rs.confidence)
}

fn is_placeholder_goal(goal: &str) -> bool {
    let trimmed = goal.trim();
    trimmed.is_empty() || trimmed.contains(PLACEHOLDER_GOAL)
}

fn deterministic_bootstrap_plan(rs: &RouteSelected, ctx: &mut LoopContext, observed: &LoopObserved) -> anyhow::Result<Option<LoopStageResult>> {
    use planning_preconditions::PlanningPrecondition;

    let preconditions = planning_preconditions::derive_preconditions_from_lines(&observed.semantic_summary.planning_preconditions);
    let needs_bootstrap = preconditions.contains(&PlanningPrecondition::MustBootstrapWorkspace);
    let needs_init = preconditions.contains(&PlanningPrecondition::MustInitCargoProject);
    if !needs_bootstrap && !needs_init {
        return Ok(None);
    }

    let target_root = observed
        .semantic_summary
        .target_root
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| observed.goal_text.as_deref().and_then(|text| parse_agent_goal_markdown(text).target_path))
        .or_else(|| ctx.goal_text.as_deref().and_then(|text| parse_agent_goal_markdown(text).target_path));
    let Some(target_root) = target_root else {
        return Ok(None);
    };

    let bootstrap_choice = if needs_bootstrap { select_bootstrap_command(&target_root) } else { BootstrapCommandChoice::CargoInit };

    let target_root_display = target_root.display().to_string();
    let target_name = target_root.file_name().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("app");
    let parent_cwd = target_root.parent().unwrap_or_else(|| Path::new("/")).display().to_string();

    let (cmd, cwd, reason, status) = match bootstrap_choice {
        BootstrapCommandChoice::CargoNew => (format!("cargo new --bin {target_name}"), parent_cwd, "deterministic_bootstrap_workspace", "deterministic_bootstrap_workspace"),
        BootstrapCommandChoice::CargoInit => ("cargo init --bin .".to_string(), target_root_display.clone(), "deterministic_init_cargo_project", "deterministic_init_cargo_project"),
        BootstrapCommandChoice::NoBootstrapNeeded => {
            return Ok(None);
        }
    };

    ctx.last_planned_observed_tick = Some(observed.tick);
    let planned_span_id = Uuid::new_v4().to_string();
    let plan_step_id = Uuid::new_v4().to_string();
    let action_id = plan_step_id.clone();
    let planned = LoopPlanned {
        tick: rs.tick,
        action_kind: "run_command".to_string(),
        action_payload: {
            let mut payload = action_payload_with_cwd(cmd, Some(cwd));
            payload["verifier"] = serde_json::Value::String("cargo_check".to_string());
            payload["failure_class"] = serde_json::Value::String(observed.semantic_summary.primary_failure_class().unwrap_or_else(|| "missing_target".to_string()));
            payload["failure_scope"] = serde_json::Value::String(observed.semantic_summary.failure_scope.clone().unwrap_or_else(|| "workspace".to_string()));
            payload
        },
        reason: reason.to_string(),
        llm_request_id: None,
        trace_id: None,
        execution_id: None,
        span_id: Some(planned_span_id),
        parent_span_id: None,
        plan_id: None,
        plan_step_id: Some(plan_step_id),
        action_id: Some(action_id),
        signals: None,
        depends_on: Vec::new(),
    };

    Ok(Some(LoopStageResult::EmitMany(vec![
        RuntimeEvent::LoopPlanned(planned),
        RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: rs.tick, llm_request_id: None, planned_count: 1, status: status.to_string() }),
    ])))
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
            LlmAction::AddImport { path, import } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "edit.add_import".to_string(),
                action_payload: serde_json::json!({ "path": path, "import": import }),
                reason: "llm_add_import".to_string(),
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
            LlmAction::DefineSymbolStub { path, symbol, kind } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "edit.define_symbol_stub".to_string(),
                action_payload: serde_json::json!({ "path": path, "symbol": symbol, "kind": kind }),
                reason: "llm_define_symbol_stub".to_string(),
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
            LlmAction::CreateModuleFile { path, module } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "edit.create_module_file".to_string(),
                action_payload: serde_json::json!({ "path": path, "module": module }),
                reason: "llm_create_module_file".to_string(),
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
            LlmAction::MoveSymbol { path, symbol_id, new_module_path } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "edit.move_symbol".to_string(),
                action_payload: serde_json::json!({ "path": path, "symbol_id": symbol_id, "new_module_path": new_module_path }),
                reason: "llm_move_symbol".to_string(),
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
            LlmAction::RenameSymbol { path, old, new } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "edit.rename_symbol".to_string(),
                action_payload: serde_json::json!({ "path": path, "old": old, "new": new }),
                reason: "llm_rename_symbol".to_string(),
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
    let retry_policy = retry_policy_for_planning_context(ctx.last_invalid_plan_reason.as_deref(), ctx.consecutive_invalid_plan_batches, &ctx.recent_execution_results, &ctx.objective_trend_state);
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
                RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: pending.tick, llm_request_id: Some(req_id), planned_count: 0, status: "missing_semantic_context".to_string() }),
            ]));
        }
    };
    let failure_scope = semantic_summary.failure_scope.clone().unwrap_or_else(|| "none".to_string());
    if let Some(failure_class) = semantic_summary.primary_failure_class() {
        for planned in &mut out {
            if planned.action_kind == "done" {
                continue;
            }
            if planned.action_payload.get("failure_class").is_none() {
                planned.action_payload["failure_class"] = serde_json::Value::String(failure_class.clone());
            }
            if planned.action_payload.get("failure_scope").is_none() {
                planned.action_payload["failure_scope"] = serde_json::Value::String(failure_scope.clone());
            }
            if planned.action_payload.get("verifier").is_none() {
                if let Some(verifier) = meta_invariant_expected_verifier(planned.action_kind.as_str(), &planned.action_payload) {
                    planned.action_payload["verifier"] = serde_json::Value::String(verifier.to_string());
                }
            }
        }
    } else {
        for planned in &mut out {
            if planned.action_kind == "done" {
                continue;
            }
            if planned.action_payload.get("verifier").is_none() {
                if let Some(verifier) = meta_invariant_expected_verifier(planned.action_kind.as_str(), &planned.action_payload) {
                    planned.action_payload["verifier"] = serde_json::Value::String(verifier.to_string());
                }
            }
        }
    }
    if let Err(message) =
        validate_action_batch(&out, retry_policy, &semantic_summary, &ctx.objective_trend_state, &ctx.recent_execution_results, ctx.forced_primary_objective, ctx.forced_primary_strategy)
    {
        let promoted_invariant = observe_failure_fingerprint(FailureFingerprint::invalid_plan_batch(Some(ConstraintRoute::Plan), planning_constraint_state(&semantic_summary)));
        ctx.last_planned_observed_tick = None;
        let mut events = vec![RuntimeEvent::InvariantDiscovered(canon_event::InvariantDiscovered { feature: "invalid_plan_batch".to_string(), confidence: 1.0, support: 1 })];
        if let Some(promotion) = promoted_invariant {
            events.push(RuntimeEvent::InvariantDiscovered(canon_event::InvariantDiscovered {
                feature: promotion.invariant.feature_name().to_string(),
                confidence: 1.0,
                support: promotion.support as u64,
            }));
        }
        for persisted in drain_persisted_store_events() {
            events.push(RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "invariant_store".to_string(),
                kind: match persisted.kind {
                    PersistedInvariantStoreEventKind::Loaded => "persisted_invariants_loaded".to_string(),
                    PersistedInvariantStoreEventKind::Updated => "persisted_invariants_updated".to_string(),
                },
                payload: serde_json::json!({
                    "path": persisted.path,
                    "support_entries": persisted.support_entries,
                    "promoted_entries": persisted.promoted_entries,
                    "reason": persisted.reason,
                }),
            }));
        }
        events.extend([
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
            RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: pending.tick, llm_request_id: Some(req_id), planned_count: 0, status: "invalid_plan".to_string() }),
        ]);
        return Ok(LoopStageResult::EmitMany(events));
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
    events.push(RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: pending.tick, llm_request_id: Some(req_id), planned_count: events.len(), status: "planned".to_string() }));
    Ok(LoopStageResult::EmitMany(events))
}

fn validate_action_batch(
    actions: &[LoopPlanned], retry_policy: RetryPolicy, semantic_summary: &SemanticStateSummary, objective_trend_state: &ObjectiveTrendState,
    recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord], forced_primary_objective: Option<DevelopmentObjectiveKind>,
    forced_primary_strategy: Option<DevelopmentStrategyKind>,
) -> Result<(), String> {
    if !semantic_summary.complete {
        return Err("semantic summary is incomplete".to_string());
    }
    let target_root = semantic_summary.target_root.as_ref().map(std::path::PathBuf::from).ok_or_else(|| "semantic summary is missing target_root".to_string())?;
    let preconditions = planning_preconditions::derive_preconditions_from_lines(&semantic_summary.planning_preconditions);
    let has_discovery = actions.iter().any(|a| matches!(a.action_kind.as_str(), "list_dir" | "read_file"));
    let has_execution = actions.iter().any(|a| {
        matches!(
            a.action_kind.as_str(),
            "patch_file"
                | "apply_patch"
                | "write_file"
                | "run_command"
                | "done"
                | "edit.rename_symbol"
                | "edit.move_symbol"
                | "edit.add_import"
                | "edit.define_symbol_stub"
                | "edit.create_module_file"
        )
    });
    if retry_policy == RetryPolicy::DiscoveryOnly && has_execution {
        return Err("discovery-only retry required after invalid plan batch; execution/edit actions are not allowed yet".to_string());
    }
    if retry_policy == RetryPolicy::SinglePatchOnly {
        let apply_patch_count = actions.iter().filter(|a| a.action_kind == "apply_patch").count();
        let has_non_patch = actions.iter().any(|a| a.action_kind != "apply_patch");
        if apply_patch_count != 1 || has_non_patch {
            return Err("single-patch retry required after apply_patch failure; emit exactly one apply_patch action and nothing else".to_string());
        }
    }
    if has_discovery && has_execution {
        return Err("mixed discovery actions with execution/edit actions in one plan batch".to_string());
    }

    if semantic_summary.primary_failure_class().is_some() && !meta_invariant_all_failures_typed(semantic_summary.failure_class.as_deref(), semantic_summary.failure_scope.as_deref()) {
        return Err("meta_invariant_all_failures_typed violated: active semantic failure must include both failure_class and failure_scope".to_string());
    }

    for action in actions {
        if let Some(expected_failure_class) = semantic_summary.primary_failure_class() {
            if action.action_kind != "done" {
                if !meta_invariant_any_action_cites_failure(&action.action_payload, Some(expected_failure_class.as_str())) {
                    let cited_failure_class = action.action_payload.get("failure_class").and_then(|v| v.as_str()).unwrap_or("<missing>");
                    return Err(format!(
                        "meta_invariant_plan_must_cite_failure violated: {} cites failure_class={} but active failure_class={}",
                        action.action_kind, cited_failure_class, expected_failure_class
                    ));
                }
            }
        }
        if meta_invariant_is_mutating_action(action.action_kind.as_str(), &action.action_payload) && !meta_invariant_action_must_declare_verifier(action.action_kind.as_str(), &action.action_payload) {
            let expected = meta_invariant_expected_verifier(action.action_kind.as_str(), &action.action_payload).unwrap_or("unknown");
            return Err(format!("meta_invariant_action_must_declare_verifier violated: {} must declare verifier={expected}", action.action_kind));
        }
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
                    return Err(format!("run_command requires an absolute cwd; got {:?}", if cwd.is_empty() { "<empty>" } else { cwd }));
                }
            }
            "edit.rename_symbol" => {
                let Some(old) = action.action_payload.get("old").and_then(|v| v.as_str()) else {
                    return Err("edit.rename_symbol missing old payload".to_string());
                };
                let Some(new) = action.action_payload.get("new").and_then(|v| v.as_str()) else {
                    return Err("edit.rename_symbol missing new payload".to_string());
                };
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err("edit.rename_symbol missing path payload".to_string());
                };
                if old.trim().is_empty() || new.trim().is_empty() {
                    return Err("edit.rename_symbol requires non-empty old and new symbol paths".to_string());
                }
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("edit.rename_symbol path is invalid: {e}"))?;
            }
            "edit.move_symbol" => {
                let Some(symbol_id) = action.action_payload.get("symbol_id").and_then(|v| v.as_str()) else {
                    return Err("edit.move_symbol missing symbol_id payload".to_string());
                };
                let Some(new_module_path) = action.action_payload.get("new_module_path").and_then(|v| v.as_str()) else {
                    return Err("edit.move_symbol missing new_module_path payload".to_string());
                };
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err("edit.move_symbol missing path payload".to_string());
                };
                if symbol_id.trim().is_empty() || new_module_path.trim().is_empty() {
                    return Err("edit.move_symbol requires non-empty symbol_id and new_module_path".to_string());
                }
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("edit.move_symbol path is invalid: {e}"))?;
            }
            "edit.add_import" => {
                let Some(import) = action.action_payload.get("import").and_then(|v| v.as_str()) else {
                    return Err("edit.add_import missing import payload".to_string());
                };
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err("edit.add_import missing path payload".to_string());
                };
                if import.trim().is_empty() {
                    return Err("edit.add_import requires non-empty import path".to_string());
                }
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("edit.add_import path is invalid: {e}"))?;
            }
            "edit.define_symbol_stub" => {
                let Some(symbol) = action.action_payload.get("symbol").and_then(|v| v.as_str()) else {
                    return Err("edit.define_symbol_stub missing symbol payload".to_string());
                };
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err("edit.define_symbol_stub missing path payload".to_string());
                };
                if symbol.trim().is_empty() {
                    return Err("edit.define_symbol_stub requires non-empty symbol".to_string());
                }
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("edit.define_symbol_stub path is invalid: {e}"))?;
            }
            "edit.create_module_file" => {
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err("edit.create_module_file missing path payload".to_string());
                };
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("edit.create_module_file path is invalid: {e}"))?;
            }
            "read_file" | "list_dir" | "write_file" | "patch_file" => {
                let Some(path) = action.action_payload.get("path").and_then(|v| v.as_str()) else {
                    return Err(format!("{} missing path payload", action.action_kind));
                };
                validate_workspace_relative_path(path, &target_root).map_err(|e| format!("{} path is invalid: {e}", action.action_kind))?;
            }
            "done" => {}
            other => {
                return Err(format!("unknown plan action_kind {other}"));
            }
        }
    }

    planning_preconditions::validate_preconditions(actions, &target_root, &preconditions, semantic_summary)?;
    let objective_state = derive_self_development_objective_state(semantic_summary, 0, recent_execution_results, objective_trend_state);
    let effective_primary_objective = forced_primary_objective.unwrap_or_else(|| primary_development_objective_kind(&objective_state, objective_trend_state, semantic_summary)).focus_text();
    let effective_primary_strategy = forced_primary_strategy.unwrap_or_else(|| primary_development_strategy_kind(&objective_state, objective_trend_state, semantic_summary));

    planning_preconditions::validate_objective_route_plan_alignment(actions, &target_root, "plan", effective_primary_objective, semantic_summary)?;
    planning_preconditions::validate_trend_intent_alignment(actions, &target_root, recent_execution_results, objective_trend_state)?;
    planning_preconditions::validate_development_strategy_alignment(actions, &target_root, semantic_summary, &objective_state, objective_trend_state, Some(effective_primary_strategy))?;

    Ok(())
}

fn planning_constraint_state(semantic_summary: &SemanticStateSummary) -> ConstraintState {
    let target_root = semantic_summary.target_root.as_deref().map(Path::new);
    let real_path_exists = target_root.is_some_and(|path| path.exists());
    let real_cargo_project = target_root.is_some_and(|path| path.join("Cargo.toml").exists());
    let failure_scope_localized = semantic_summary.failure_scope.as_deref() == Some("localized");
    let failure_scope_workspace = semantic_summary.failure_scope.as_deref() == Some("workspace");
    let failure_scope_tooling = semantic_summary.failure_scope.as_deref() == Some("tooling");
    ConstraintState {
        semantic_path_exists: semantic_summary.path_exists,
        semantic_cargo_project: semantic_summary.cargo_project,
        real_path_exists,
        real_cargo_project,
        actionable_failure: meta_invariant_has_actionable_failure(
            semantic_summary.validation_blocked_by_preconditions,
            semantic_summary.compiler_repair_required,
            semantic_summary.planning_preconditions.len(),
            semantic_summary.compiler_hints.len(),
            semantic_summary.module_gaps.len(),
        ),
        validation_blocked: semantic_summary.validation_blocked_by_preconditions,
        entrypoint_missing: matches!(semantic_summary.entrypoint_kind.as_deref(), Some("none") | None) && semantic_summary.cargo_project,
        module_gaps_present: !semantic_summary.module_gaps.is_empty(),
        recent_no_semantic_progress: false,
        failure_class_no_actionable: semantic_summary.primary_failure_class().as_deref() == Some("no_actionable_failure"),
        failure_scope_localized,
        failure_scope_workspace,
        failure_scope_tooling,
        route_objective_contradiction: false,
    }
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
    Ok(LoopStageResult::Emit(RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: pending.tick, llm_request_id: Some(pending.request_id), planned_count: 0, status: "llm_failed".to_string() })))
}

fn handle_observed(ctx: &mut LoopContext, observed: &LoopObserved, trigger_id: EventId, route_rationale: Option<String>, route_confidence: Option<f32>) -> anyhow::Result<LoopStageResult> {
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
                RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: observed.tick, llm_request_id: None, planned_count: 0, status: "missing_semantic_context".to_string() }),
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
            return Ok(LoopStageResult::Emit(RuntimeEvent::PlanningCompleted(PlanningCompleted { tick: observed.tick, llm_request_id: None, planned_count: 0, status: "goal_complete".to_string() })));
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
    let target_workspace = semantic_summary.target_root.clone().unwrap_or_else(|| workspace_clone.display().to_string());
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
        ctx.forced_primary_objective,
        ctx.forced_primary_strategy,
    );
    let context_base = build_context_base(observed, &workspace_clone, &sub_agent_section, &llm_semantic_context);
    let context_base_hash = hash_str(&context_base);

    let context_delta = build_context_delta(&llm_semantic_context, &ctx.batch_acted, ctx.last_invalid_plan_reason.as_deref(), ctx.consecutive_invalid_plan_batches);

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
        emitter.emit_with_parents(
            RuntimeEvent::ToolCall(ToolCall {
                node_id: "plan_consumer".to_string(),
                tool_call_id: plan_tool_call_id,
                request_id: request_id.clone(),
                kind: "llm.plan".to_string(),
                payload: serde_json::json!({"role": "planner"}),
                accepted: true,
            }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
        emitter.emit_with_parents(
            RuntimeEvent::Llm(LlmCall {
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
            }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
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
    Some(PlanningCompleted { tick, llm_request_id: None, planned_count: 0, status: "llm_timeout".to_string() })
}

fn emit_tool_result(ctx: &LoopContext, tool_call_id: &str, request_id: &str, success: bool, trigger_id: &EventId) -> anyhow::Result<()> {
    if let Some(emitter) = ctx.emitter.as_ref() {
        emitter.emit_with_parents(
            RuntimeEvent::ToolResult(ToolResult {
                node_id: "plan_consumer".to_string(),
                tool_call_id: tool_call_id.to_string(),
                tool_result_id: Uuid::new_v4().to_string(),
                request_id: request_id.to_string(),
                kind: "llm.plan".to_string(),
                output: serde_json::json!({}),
                success,
            }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
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
    AddImport { path: String, import: String },
    DefineSymbolStub { path: String, symbol: String, kind: String },
    CreateModuleFile { path: String, module: Option<String> },
    MoveSymbol { path: String, symbol_id: String, new_module_path: String },
    RenameSymbol { path: String, old: String, new: String },
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

3. apply_patch — create, update, or delete files
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

4. edit.rename_symbol — perform a semantic rename via the editor stack when graph-backed strategy calls for it
   {"action":"edit.rename_symbol","old":"crate::module::OldName","new":"crate::module::NewName","path":"src/module.rs"}
   Use this for duplicate-definition / rename flows when you have graph-backed symbol context.

5. edit.move_symbol — move a symbol to a different module using the semantic editor stack
   {"action":"edit.move_symbol","symbol_id":"crate::old_mod::Thing","new_module_path":"crate::new_mod","path":"src/old_mod.rs"}
   Use this for module-restructure / cohesion strategies driven by graph hotspots.

6. edit.add_import — add a semantic import to an existing Rust file
   {"action":"edit.add_import","import":"crate::foo::Bar","path":"src/lib.rs"}
   Default tool for unresolved-import repairs.

7. edit.define_symbol_stub — add a semantic stub for a missing symbol
   {"action":"edit.define_symbol_stub","symbol":"run","kind":"fn","path":"src/lib.rs"}
   Default tool for missing-symbol repairs.

8. edit.create_module_file — create a declared missing module file directly
   {"action":"edit.create_module_file","module":"merge","path":"src/merge.rs"}
   Default tool for missing-module repairs.

9. run_command — run a shell command
   {"action":"run_command","cmd":"cargo build","cwd":"<TARGET_WORKSPACE>"}
   cwd must be absolute. Use TARGET WORKSPACE (provided in context) or a subdir.

10. done — declare goal complete
   {"action":"done","reason":"..."}

━━━ WORKFLOW ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Step 1 — Discover (only when unsure of project state or missing file contents):
  Emit ONLY list_dir and/or read_file. Do NOT mix with edits.
  → Results appear in "Recent actions" on your next call.
  Bootstrap exception:
  - If the semantic summary says `path_exists=false`,
    `validation_blocked=true`, or planning preconditions include
    `must_bootstrap_workspace=true`, do NOT emit discovery first.
  - In that case, the first valid batch is a bootstrap batch that creates
    the target workspace directly with exactly one `run_command`.
  - Prefer:
    `mkdir -p <TARGET_WORKSPACE> && cargo init --name <crate_name> --bin <TARGET_WORKSPACE>`
    when the directory path already exists or may already exist.
  - Use:
    `cargo new --bin <TARGET_WORKSPACE> --name <crate_name>`
    only when creating a brand-new directory path.

Step 2 — Create/Edit (after seeing discovery results):
  Use semantic editor actions first for covered compiler repairs.
  - edit.rename_symbol for duplicate-definition repairs
  - edit.move_symbol for module restructuring
  - edit.add_import for unresolved imports
  - edit.define_symbol_stub for missing symbols
  - edit.create_module_file for missing modules
  Use apply_patch only for edits not covered by the semantic editor stack.
  Use run_command for cargo/shell operations.
  The "done" action must be the ONLY action in a batch, and only after verification has shown the goal is met.

NEVER use "write" or "patch_file" — they are removed. Use apply_patch.
NEVER assume a directory/project exists without checking with list_dir first.
EXCEPTION: when bootstrap is explicitly required for a missing target workspace,
create the workspace first with `run_command`; discovery comes after bootstrap succeeds.
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
static PLANNER_SYSTEM_PROMPT_ID: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| hash_str(PLANNER_SYSTEM_INSTRUCTIONS));

/// Tier-2 context: slow-changing section containing GOAL and workspace state.
/// Sent only when its hash differs from `ctx.last_context_base_id`. For stateful
/// endpoints the LLM already has this in session history; for stateless endpoints
/// the executor worker reconstructs it from its cache before each API call.
fn build_context_base(observed: &LoopObserved, workspace: &Path, sub_agent_section: &str, llm_semantic_context: &LlmSemanticContext) -> String {
    let goal_text = observed.goal_text.clone().unwrap_or_else(|| "<no goal provided>".to_string());
    let target_workspace = llm_semantic_context.target_workspace.clone().or_else(|| llm_semantic_context.semantic_summary.target_root.clone()).unwrap_or_else(|| workspace.display().to_string());
    let semantic_planner_block = llm_semantic_context.render_planner_base_block();
    let planner_skill_block = build_planner_skill_block(llm_semantic_context);
    let graph_strategy_block = build_graph_strategy_block(llm_semantic_context);
    let semantic_repair_block = build_semantic_repair_block(llm_semantic_context);

    let search_hints = build_search_hints(&goal_text, workspace);
    let workspace_tree = build_workspace_tree(std::path::Path::new(&target_workspace), 3, 0);

    format!(
        r#"GOAL:
{goal_text}

## Workspace State
{workspace_tree}

{semantic_planner_block}

{planner_skill_block}

{graph_strategy_block}

{semantic_repair_block}

━━━ CONTEXT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Relevant files:{search_hints}

{sub_agent_section}"#,
        goal_text = goal_text,
        semantic_planner_block = semantic_planner_block,
        planner_skill_block = planner_skill_block,
        graph_strategy_block = graph_strategy_block,
        semantic_repair_block = semantic_repair_block,
        workspace_tree = workspace_tree,
        search_hints = search_hints,
        sub_agent_section = sub_agent_section,
    )
}

fn build_planner_skill_block(llm_semantic_context: &LlmSemanticContext) -> String {
    let objective = primary_development_objective_kind(&llm_semantic_context.objective_state, &llm_semantic_context.objective_trend_state, &llm_semantic_context.semantic_summary);
    let strategy = primary_development_strategy_kind(&llm_semantic_context.objective_state, &llm_semantic_context.objective_trend_state, &llm_semantic_context.semantic_summary);
    let registry = global_registry();
    let Ok(skills) = registry.select_for_scope("planner", objective, strategy) else {
        return "Planner skills:\n- none".to_string();
    };
    if skills.is_empty() {
        return "Planner skills:\n- none".to_string();
    }
    let rendered = skills.into_iter().map(|skill| format!("### Skill: {}\n{}", skill.name, skill.prompt.trim())).collect::<Vec<_>>().join("\n\n");
    format!("Planner skills:\n{rendered}")
}

fn build_graph_strategy_block(llm_semantic_context: &LlmSemanticContext) -> String {
    let strategy = primary_development_strategy_kind(&llm_semantic_context.objective_state, &llm_semantic_context.objective_trend_state, &llm_semantic_context.semantic_summary);
    let Some(target_workspace) = llm_semantic_context.target_workspace.as_deref().or(llm_semantic_context.semantic_summary.target_root.as_deref()) else {
        return "Graph strategy hints:\n- none".to_string();
    };
    let workspace = Path::new(target_workspace);
    match strategy {
        canon_semantic_state::DevelopmentStrategyKind::PlanSymbolAwareRename => match graph_backed_rename_candidates(workspace, 3) {
            Ok(candidates) if !candidates.is_empty() => {
                let lines = candidates
                    .into_iter()
                    .map(|candidate| {
                        let path = candidate.file_path.unwrap_or_else(|| "src/lib.rs".to_string());
                        format!(
                            "- rename candidate: `{}` -> `{}`\n  suggested action: {{\"action\":\"edit.rename_symbol\",\"old\":\"{}\",\"new\":\"{}\",\"path\":\"{}\"}}",
                            candidate.symbol_path, candidate.suggested_path, candidate.symbol_path, candidate.suggested_path, path
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Graph strategy hints:\n{lines}")
            }
            _ => "Graph strategy hints:\n- none".to_string(),
        },
        canon_semantic_state::DevelopmentStrategyKind::RestructureModules => match graph_backed_module_moves(workspace, 3) {
            Ok(candidates) if !candidates.is_empty() => {
                let lines = candidates
                    .into_iter()
                    .map(|candidate| {
                        let path = candidate.file_path.unwrap_or_else(|| "src/lib.rs".to_string());
                        format!(
                            "- module hotspot move: `{}` -> `{}`\n  suggested action: {{\"action\":\"edit.move_symbol\",\"symbol_id\":\"{}\",\"new_module_path\":\"{}\",\"path\":\"{}\"}}",
                            candidate.symbol_path, candidate.to_module_path, candidate.symbol_path, candidate.to_module_path, path
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Graph strategy hints:\n{lines}")
            }
            _ => "Graph strategy hints:\n- none".to_string(),
        },
        _ => "Graph strategy hints:\n- none".to_string(),
    }
}

fn build_semantic_repair_block(llm_semantic_context: &LlmSemanticContext) -> String {
    let mut lines = Vec::new();
    for gap in &llm_semantic_context.semantic_summary.module_gaps {
        if let Some((module, path)) = gap.split_once(" -> ") {
            lines.push(format!("- missing module `{}`\n  suggested action: {{\"action\":\"edit.create_module_file\",\"module\":\"{}\",\"path\":\"{}\"}}", module.trim(), module.trim(), path.trim()));
        }
    }

    for hint in &llm_semantic_context.semantic_summary.compiler_hints {
        let Some(target) = hint.target_files.first() else {
            continue;
        };
        match hint.kind_enum() {
            Some(canon_semantic_state::CompilerHintKind::UnresolvedImport) => {
                if let Some(import_path) = extract_backticked_symbol(&hint.summary) {
                    lines.push(format!("- unresolved import `{}`\n  suggested action: {{\"action\":\"edit.add_import\",\"import\":\"{}\",\"path\":\"{}\"}}", import_path, import_path, target));
                }
            }
            Some(canon_semantic_state::CompilerHintKind::MissingSymbol) => {
                if let Some(symbol) = extract_backticked_symbol(&hint.summary) {
                    lines.push(format!(
                        "- missing symbol `{}`\n  suggested action: {{\"action\":\"edit.define_symbol_stub\",\"symbol\":\"{}\",\"kind\":\"fn\",\"path\":\"{}\"}}",
                        symbol, symbol, target
                    ));
                }
            }
            _ => {}
        }
    }

    if lines.is_empty() {
        "Semantic repair hints:\n- none".to_string()
    } else {
        format!("Semantic repair hints:\n{}", lines.join("\n"))
    }
}

fn extract_backticked_symbol(text: &str) -> Option<String> {
    let start = text.find('`')?;
    let tail = &text[start + 1..];
    let end = tail.find('`')?;
    let value = tail[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Tier-3 context: fast-changing delta sent on every planning call.
/// Contains only the fields that change after each action: LOC, error counts,
/// recent actions and tool results. Does NOT include GOAL or workspace tree.
fn build_context_delta(llm_semantic_context: &LlmSemanticContext, batch_acted: &[LoopActed], last_invalid_plan_reason: Option<&str>, consecutive_invalid_plan_batches: u32) -> String {
    let destructive_warning = batch_acted.iter().any(|a| a.stderr.trim() == "rejected_destructive_command");
    let destructive_note = if destructive_warning { "WARNING: A previous plan was blocked as destructive. Do NOT include destructive commands; they will fail.\n" } else { "" };

    let invalid_plan_section = match last_invalid_plan_reason {
        Some(reason) => {
            let policy = retry_policy_for_planning_context(Some(reason), consecutive_invalid_plan_batches, &llm_semantic_context.recent_execution_results, &llm_semantic_context.objective_trend_state);
            let policy_text = retry_policy_text(policy, true);
            format!("{}\n{policy_text}", llm_semantic_context.render_planner_delta_block())
        }
        None => {
            let policy = retry_policy_for_planning_context(None, consecutive_invalid_plan_batches, &llm_semantic_context.recent_execution_results, &llm_semantic_context.objective_trend_state);
            if policy == RetryPolicy::CorrectiveRetry {
                format!("{}\n{}", llm_semantic_context.render_planner_delta_block(), retry_policy_text(policy, false))
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
        &llm_semantic_context.semantic_summary,
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
    semantic_summary: &SemanticStateSummary, observed: &LoopObserved, batch_acted: &[LoopActed], batch_tool_results: &[ToolResult],
    recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord], target_workspace: &str, route_rationale: Option<&str>, route_confidence: Option<f64>,
    last_invalid_plan_reason: Option<&str>, last_invalid_plan_planned_count: Option<usize>, consecutive_invalid_plan_batches: u32, objective_trend_state: &ObjectiveTrendState,
    forced_primary_objective: Option<DevelopmentObjectiveKind>, forced_primary_strategy: Option<DevelopmentStrategyKind>,
) -> LlmSemanticContext {
    let recent_actions = batch_acted
        .iter()
        .rev()
        .take(24)
        .map(|action| {
            let mut entry = format!("- action={} success={} exit_code={:?}", action.action_kind, action.success, action.exit_code);
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
            let content = serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| result.output.to_string());
            let truncated = if content.len() > 600 { &content[..600] } else { &content };
            format!("- kind={} success={}\n  output: {}", result.kind, result.success, truncated)
        })
        .collect::<Vec<_>>();
    LlmSemanticContext {
        mission_summary: observed.goal_text.as_deref().map(parse_agent_goal_markdown).map(|goal| canon_goal::summarize_goal(&goal)),
        semantic_summary: semantic_summary.clone(),
        objective_state: derive_self_development_objective_state(semantic_summary, consecutive_invalid_plan_batches, recent_execution_results, objective_trend_state),
        objective_trend_state: objective_trend_state.clone(),
        forced_primary_objective,
        forced_primary_strategy,
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
    batch_acted: &[LoopActed], last_invalid_plan_reason: Option<&str>, consecutive_invalid_plan_batches: u32, recent_execution_results: &[canon_semantic_state::SemanticExecutionResultRecord],
    objective_trend_state: &ObjectiveTrendState, semantic_summary: &SemanticStateSummary,
) -> String {
    let last_failure = if recent_execution_results.is_empty() {
        batch_acted
            .iter()
            .rev()
            .find(|a| !a.success && (!a.stderr.trim().is_empty() || !a.stdout.trim().is_empty()))
            .map(|a| (a.action_kind.clone(), if !a.stderr.trim().is_empty() { a.stderr.clone() } else { a.stdout.clone() }))
    } else {
        None
    };
    let mut hint_lines = planner_hint_lines(
        last_invalid_plan_reason,
        consecutive_invalid_plan_batches,
        recent_execution_results,
        objective_trend_state,
        last_failure.as_ref().map(|(kind, _)| kind.as_str()),
        last_failure.as_ref().map(|(_, text)| truncate_hint_text(text, 240)).as_deref(),
    );
    hint_lines.extend(semantic_planner_hint_lines(semantic_summary.primary_failure_class().as_deref(), semantic_summary.failure_scope.as_deref()));
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
            let skip = path.file_name().and_then(|n| n.to_str()).map(|n| matches!(n, "target" | ".git" | "node_modules" | ".cargo")).unwrap_or(false);
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
            "edit.rename_symbol" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let old = value.get("old").and_then(|v| v.as_str())?;
                let new = value.get("new").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::RenameSymbol { path: path.to_string(), old: old.to_string(), new: new.to_string() }, depends_on });
            }
            "edit.move_symbol" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let symbol_id = value.get("symbol_id").and_then(|v| v.as_str())?;
                let new_module_path = value.get("new_module_path").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::MoveSymbol { path: path.to_string(), symbol_id: symbol_id.to_string(), new_module_path: new_module_path.to_string() }, depends_on });
            }
            "edit.add_import" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let import = value.get("import").and_then(|v| v.as_str())?;
                return Some(ActionPlan { action: LlmAction::AddImport { path: path.to_string(), import: import.to_string() }, depends_on });
            }
            "edit.define_symbol_stub" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let symbol = value.get("symbol").and_then(|v| v.as_str())?;
                let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("fn");
                return Some(ActionPlan { action: LlmAction::DefineSymbolStub { path: path.to_string(), symbol: symbol.to_string(), kind: kind.to_string() }, depends_on });
            }
            "edit.create_module_file" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let module = value.get("module").and_then(|v| v.as_str()).map(ToString::to_string);
                return Some(ActionPlan { action: LlmAction::CreateModuleFile { path: path.to_string(), module }, depends_on });
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

#[cfg(test)]
mod tests {
    use super::{build_graph_strategy_block, build_semantic_repair_block, validate_action_batch};
    use canon_event::LoopPlanned;
    use canon_ir::{csr_graph::CsrGraph, CanonIR, CanonNodeKind};
    use canon_semantic_state::{derive_self_development_objective_state, CompilerHintKind, CompilerHintRecord, DevelopmentStrategyKind, LlmSemanticContext, ObjectiveTrendState, SemanticStateSummary};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn temp_workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!("canon-loop-{}", Uuid::new_v4()));
        fs::create_dir_all(path.join("src")).unwrap();
        path
    }

    fn write_latest_graph_artifact(workspace: &Path, ir: &CanonIR) {
        let artifact_id = Uuid::new_v4().simple().to_string();
        let artifact_dir = workspace.join("state").join("graph");
        fs::create_dir_all(artifact_dir.join("index").join("by_crate")).unwrap();
        fs::create_dir_all(artifact_dir.join("index").join("by_hash")).unwrap();
        let artifact_path = artifact_dir.join(format!("{artifact_id}.json"));
        fs::write(&artifact_path, serde_json::to_vec(ir).unwrap()).unwrap();
        let summary = canon_analysis::GraphArtifactSummary {
            artifact_id: artifact_id.clone(),
            artifact_path: artifact_path.clone(),
            crate_name: "example".to_string(),
            node_count: ir.nodes.len(),
            edge_count: ir.module_graph.edge_count() + ir.call_graph.edge_count() + ir.cfg_graph.edge_count(),
            file_count: 2,
            call_edge_count: ir.call_graph.edge_count(),
            module_edge_count: ir.module_graph.edge_count(),
            cfg_edge_count: ir.cfg_graph.edge_count(),
        };
        let index = canon_analysis::GraphArtifactIndex { latest_workspace: summary.clone() };
        fs::write(artifact_dir.join("index").join("latest_workspace.json"), serde_json::to_vec(&index).unwrap()).unwrap();
        fs::write(artifact_dir.join("index").join("by_crate").join("example.json"), serde_json::to_vec(&summary).unwrap()).unwrap();
        fs::write(artifact_dir.join("index").join("by_hash").join(format!("{artifact_id}.json")), serde_json::to_vec(&summary).unwrap()).unwrap();
    }

    fn rename_ir() -> CanonIR {
        let mut ir = CanonIR::new();
        let mod_alpha = ir.intern_path("crate::alpha").unwrap();
        let mod_beta = ir.intern_path("crate::beta").unwrap();
        let foo = ir.intern_name("Foo");
        let alpha_id = ir.push_node(CanonNodeKind::Module { path_id: mod_alpha, flags: 0 });
        let beta_id = ir.push_node(CanonNodeKind::Module { path_id: mod_beta, flags: 0 });
        let foo_alpha = ir.push_node(CanonNodeKind::Struct { name_id: foo, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 });
        let foo_beta = ir.push_node(CanonNodeKind::Struct { name_id: foo, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 });
        let node_data = ir.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        ir.module_graph = CsrGraph::from_edges(node_data.clone(), vec![(alpha_id.0, foo_alpha.0, canon_ir::EdgeKind::Contains), (beta_id.0, foo_beta.0, canon_ir::EdgeKind::Contains)]);
        ir.call_graph = CsrGraph::from_edges(node_data.clone(), Vec::new());
        ir.cfg_graph = CsrGraph::from_edges(node_data, Vec::new());
        ir
    }

    fn restructure_ir() -> CanonIR {
        let mut ir = CanonIR::new();
        let mod_alpha = ir.intern_path("crate::alpha").unwrap();
        let mod_beta = ir.intern_path("crate::beta").unwrap();
        let worker = ir.intern_name("Worker");
        let caller = ir.intern_name("call_worker");
        let alpha_id = ir.push_node(CanonNodeKind::Module { path_id: mod_alpha, flags: 0 });
        let beta_id = ir.push_node(CanonNodeKind::Module { path_id: mod_beta, flags: 0 });
        let worker_id = ir.push_node(CanonNodeKind::Struct { name_id: worker, generics: Vec::new(), fields: Vec::new(), derives: Vec::new(), attrs: Vec::new(), flags: 0, struct_kind: 0 });
        let caller_id = ir.push_node(CanonNodeKind::Fn { name_id: caller, sig_id: worker_id, body: None, attrs: Vec::new(), flags: 0 });
        let node_data = ir.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        ir.module_graph = CsrGraph::from_edges(
            node_data.clone(),
            vec![
                (alpha_id.0, worker_id.0, canon_ir::EdgeKind::Contains),
                (beta_id.0, caller_id.0, canon_ir::EdgeKind::Contains),
                (alpha_id.0, beta_id.0, canon_ir::EdgeKind::Reexports),
                (alpha_id.0, beta_id.0, canon_ir::EdgeKind::Reexports),
                (alpha_id.0, beta_id.0, canon_ir::EdgeKind::Reexports),
                (alpha_id.0, beta_id.0, canon_ir::EdgeKind::Reexports),
                (alpha_id.0, beta_id.0, canon_ir::EdgeKind::Reexports),
            ],
        );
        ir.call_graph = CsrGraph::from_edges(node_data.clone(), vec![(caller_id.0, worker_id.0, canon_ir::EdgeKind::Calls)]);
        ir.cfg_graph = CsrGraph::from_edges(node_data, Vec::new());
        ir
    }

    fn context_for_strategy(workspace: &Path, semantic_summary: SemanticStateSummary, trend: ObjectiveTrendState) -> LlmSemanticContext {
        let objective_state = derive_self_development_objective_state(&semantic_summary, 0, &[], &trend);
        LlmSemanticContext {
            mission_summary: None,
            semantic_summary,
            objective_state,
            objective_trend_state: trend,
            forced_primary_objective: None,
            forced_primary_strategy: None,
            target_workspace: Some(workspace.display().to_string()),
            workspace_loc: None,
            error_count: None,
            warning_count: None,
            route_rationale: None,
            route_confidence: None,
            invalid_plan_reason: None,
            invalid_plan_planned_count: None,
            consecutive_invalid_plan_batches: 0,
            low_level_diagnostics: Vec::new(),
            recent_actions: Vec::new(),
            recent_tool_results: Vec::new(),
            recent_execution_results: Vec::new(),
        }
    }

    #[test]
    fn graph_strategy_block_prefers_semantic_rename_payloads() {
        let workspace = temp_workspace();
        fs::write(workspace.join("src").join("alpha.rs"), "pub struct Foo;\n").unwrap();
        fs::write(workspace.join("src").join("beta.rs"), "pub struct Foo;\n").unwrap();
        write_latest_graph_artifact(&workspace, &rename_ir());
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            graph_artifact_id: Some("artifact".into()),
            compiler_hints: vec![CompilerHintRecord::new(CompilerHintKind::DuplicateDefinition, "duplicate definition", "use semantic rename", vec!["src/lib.rs".into()])],
            ..SemanticStateSummary::default()
        };
        let block = build_graph_strategy_block(&context_for_strategy(&workspace, semantic_summary, ObjectiveTrendState::default()));
        assert!(block.contains("\"action\":\"edit.rename_symbol\""));
        assert!(!block.contains("\"action\":\"apply_patch\""));
        assert!(block.contains("\"path\":\"src/alpha.rs\"") || block.contains("\"path\":\"src/beta.rs\""));
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn graph_strategy_block_emits_move_symbol_payloads() {
        let workspace = temp_workspace();
        fs::write(workspace.join("src").join("alpha.rs"), "pub struct Worker;\n").unwrap();
        fs::write(workspace.join("src").join("beta.rs"), "pub fn call_worker() {}\n").unwrap();
        write_latest_graph_artifact(&workspace, &restructure_ir());
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            graph_artifact_id: Some("artifact".into()),
            rust_file_count: Some(12),
            graph_module_edge_count: Some(48),
            graph_call_edge_count: Some(1),
            source_files: vec!["tests/cohesion_test.rs".into()],
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState { baseline_module_gap_count: Some(0), current_module_gap_count: Some(3), ..ObjectiveTrendState::default() };
        let block = build_graph_strategy_block(&context_for_strategy(&workspace, semantic_summary, trend));
        assert!(block.contains("\"action\":\"edit.move_symbol\""));
        assert!(block.contains("\"new_module_path\":\"crate::beta\""));
        assert!(block.contains("\"path\":\"src/alpha.rs\""));
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn validate_action_batch_accepts_semantic_rename_action() {
        let workspace = temp_workspace();
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            failure_class: Some("duplicate_definition".into()),
            failure_scope: Some("localized".into()),
            graph_artifact_id: Some("artifact".into()),
            compiler_hints: vec![CompilerHintRecord::new(CompilerHintKind::DuplicateDefinition, "duplicate definition", "rename duplicate", vec!["src/alpha.rs".into()])],
            ..SemanticStateSummary::default()
        };
        let rename = LoopPlanned {
            tick: 1,
            action_kind: "edit.rename_symbol".to_string(),
            action_payload: json!({
                "old": "crate::alpha::Foo",
                "new": "crate::alpha::FooAlpha",
                "path": "src/alpha.rs",
                "failure_class": "duplicate_definition",
                "verifier": "graph_proof"
            }),
            reason: "semantic rename".to_string(),
            llm_request_id: None,
            signals: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            depends_on: Vec::new(),
        };
        assert!(validate_action_batch(&[rename], crate::policy::RetryPolicy::CorrectiveRetry, &semantic_summary, &ObjectiveTrendState::default(), &[], None, None,).is_ok());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn validate_action_batch_accepts_move_symbol_action() {
        let workspace = temp_workspace();
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            failure_class: Some("missing_module".into()),
            failure_scope: Some("localized".into()),
            graph_artifact_id: Some("artifact".into()),
            rust_file_count: Some(12),
            graph_module_edge_count: Some(48),
            graph_call_edge_count: Some(1),
            source_files: vec!["tests/cohesion_test.rs".into()],
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState { baseline_module_gap_count: Some(0), current_module_gap_count: Some(3), ..ObjectiveTrendState::default() };
        let move_symbol = LoopPlanned {
            tick: 1,
            action_kind: "edit.move_symbol".to_string(),
            action_payload: json!({
                "symbol_id": "crate::alpha::Worker",
                "new_module_path": "crate::beta",
                "path": "src/alpha.rs",
                "failure_class": "missing_module",
                "verifier": "graph_proof"
            }),
            reason: "module restructure".to_string(),
            llm_request_id: None,
            signals: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            depends_on: Vec::new(),
        };
        assert!(validate_action_batch(&[move_symbol], crate::policy::RetryPolicy::CorrectiveRetry, &semantic_summary, &trend, &[], None, None,).is_ok());
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn validate_action_batch_rejects_missing_verifier_for_mutation() {
        let workspace = temp_workspace();
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            failure_class: Some("duplicate_definition".into()),
            failure_scope: Some("localized".into()),
            ..SemanticStateSummary::default()
        };
        let rename = LoopPlanned {
            tick: 1,
            action_kind: "edit.rename_symbol".to_string(),
            action_payload: json!({
                "old": "crate::alpha::Foo",
                "new": "crate::alpha::FooAlpha",
                "path": "src/alpha.rs",
                "failure_class": "duplicate_definition"
            }),
            reason: "semantic rename".to_string(),
            llm_request_id: None,
            signals: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            depends_on: Vec::new(),
        };
        let result = validate_action_batch(&[rename], crate::policy::RetryPolicy::CorrectiveRetry, &semantic_summary, &ObjectiveTrendState::default(), &[], None, None);
        assert!(result.unwrap_err().contains("meta_invariant_action_must_declare_verifier"));
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn graph_rename_strategy_is_selected_for_duplicate_definition_context() {
        let summary = SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            graph_artifact_id: Some("artifact".into()),
            compiler_hints: vec![CompilerHintRecord::new(CompilerHintKind::DuplicateDefinition, "duplicate definition", "rename duplicate", vec!["src/lib.rs".into()])],
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState::default();
        let objective_state = derive_self_development_objective_state(&summary, 0, &[], &trend);
        assert_eq!(canon_semantic_state::primary_development_strategy_kind(&objective_state, &trend, &summary), DevelopmentStrategyKind::PlanSymbolAwareRename);
    }

    #[test]
    fn semantic_repair_block_prefers_editor_actions_for_local_repairs() {
        let workspace = temp_workspace();
        let semantic_summary = SemanticStateSummary {
            complete: true,
            target_root: Some(workspace.display().to_string()),
            path_exists: true,
            cargo_project: true,
            module_gaps: vec!["merge -> src/merge.rs".into()],
            compiler_hints: vec![
                CompilerHintRecord::new(CompilerHintKind::UnresolvedImport, "compiler reports unresolved import `crate::foo`", "add import", vec!["src/lib.rs".into()]),
                CompilerHintRecord::new(CompilerHintKind::MissingSymbol, "compiler cannot find `run` in scope", "define symbol", vec!["src/main.rs".into()]),
            ],
            ..SemanticStateSummary::default()
        };
        let block = build_semantic_repair_block(&context_for_strategy(&workspace, semantic_summary, ObjectiveTrendState::default()));
        assert!(block.contains("\"action\":\"edit.create_module_file\""));
        assert!(block.contains("\"action\":\"edit.add_import\""));
        assert!(block.contains("\"action\":\"edit.define_symbol_stub\""));
        let _ = fs::remove_dir_all(workspace);
    }
}
