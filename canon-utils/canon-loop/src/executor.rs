use crate::stage::observe;
use crate::{
    context::LoopContext,
    harness_repair::{evaluate_harness_repair_loop, HarnessRepairDecision},
    policy::{
        classify_action_outcome, evaluate_loop_runtime, evaluate_loop_transition, evaluate_recovery_event,
        evaluate_recovery_execution, evaluate_error_observe, evaluate_bootstrap_effects,
        ErrorObserveRule, LoopRecoveryRule, ObserveExecutionMode,
        RecoveryEventRule, RecoveryOperation, StageExecutionOutcomeClass,
    },
    result::LoopStageResult,
    scheduler::{infer_priority, ScheduledTask},
    stage::LoopStageEvent,
};
use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalEdgeDefined, RuntimeEvent, Tick};
use canon_invariant::{
    decision_trace_payload, evaluate_constraint_context, meta_invariant_action_must_declare_verifier,
    meta_invariant_any_action_cites_failure,
    meta_invariant_classify_planned_action_class, meta_invariant_expected_verifier,
    meta_invariant_has_actionable_failure,
    meta_invariant_no_progress_forces_change, meta_invariant_verifier_sequence_contract,
    ConstraintAction, ConstraintContext, ConstraintDecision, ConstraintState,
    MetaInvariantVerifierSequenceStep,
};
use canon_proc_macros::must_emit;
use canon_semantic_state::{classify_planned_action_intents, execution_results_for_action, SemanticExecutionResultRecord};
use std::path::PathBuf;
use std::time::Instant;

pub struct LoopStageExecutor {
    ctx: LoopContext,
}

impl LoopStageExecutor {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self { ctx: LoopContext::new(workspace, tlog_path) }
    }

    pub fn with_agent_id(mut self, id: String) -> Self {
        self.ctx.agent_id = Some(id);
        self
    }

    pub fn evaluate_harness_repair(&self) -> HarnessRepairDecision {
        evaluate_harness_repair_loop(&self.ctx.harness_repair_state())
    }

    pub fn evaluate_harness_repair_for_target(
        &mut self,
        target: &crate::harness_repair::HarnessRepairTarget,
        failure_output: &str,
    ) -> crate::harness_repair::HarnessRepairDirective {
        self.ctx.prime_harness_repair_target(target, failure_output);
        crate::harness_repair::build_harness_repair_directive(&self.ctx.harness_repair_state(), target)
    }

    fn record_control_state(&mut self, event: &RuntimeEvent, trigger_id: &EventId) {
        let next = match event {
            RuntimeEvent::RouteSelected(rs) => match rs.approved_route.as_str() {
                "observe" => Some("loop_observed"),
                "plan" => Some("planning_completed"),
                "act" => Some("loop_acted"),
                "verify" => Some("verifier_policy_updated"),
                "conclude" => Some("loop_rewarded"),
                _ => None,
            },
            RuntimeEvent::LoopObserved(_) => Some("route_selected"),
            RuntimeEvent::PlanningCompleted(_) => Some("route_selected"),
            RuntimeEvent::LoopActed(_) => Some("route_selected"),
            RuntimeEvent::LoopVerified(_) => Some("verifier_policy_updated"),
            RuntimeEvent::VerifierPolicyUpdated(_) => Some("loop_rewarded"),
            RuntimeEvent::LoopRewarded(_) => Some("route_selected"),
            _ => None,
        };
        if let Some(expected) = next {
            self.ctx.last_control_event_id = Some(trigger_id.to_string());
            self.ctx.last_control_kind = Some(canon_event::event_kind_str(event).to_string());
            self.ctx.pending_required_successor = Some(expected.to_string());
        }
    }

    fn consume_control_successor(&mut self, event: &RuntimeEvent) {
        let event_kind = canon_event::event_kind_str(event);
        if self.ctx.pending_required_successor.as_deref() == Some(event_kind) {
            self.ctx.pending_required_successor = None;
        }
    }
    fn emit_debug(&self, trigger_id: &EventId, kind: &str, reason: &str, payload: serde_json::Value) {
        if let Some(emitter) = self.ctx.emitter.as_ref() {
            emitter.emit_child(
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "loop_stage_executor".to_string(),
                    kind: kind.to_string(),
                    payload: decision_trace_payload(reason, payload),
                }),
                vec![trigger_id.clone()],
                file!(),
                line!(),
            );
        }
    }

    fn emit_error(
        &self,
        trigger_id: &EventId,
        kind: &str,
        message: String,
        severity: &str,
        context: serde_json::Value,
    ) {
        if let Some(emitter) = self.ctx.emitter.as_ref() {
            emitter.emit_child(
                RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                    kind,
                    "loop_stage_executor",
                    message,
                    severity,
                    context,
                    None,
                )),
                vec![trigger_id.clone()],
                file!(),
                line!(),
            );
        }
    }

    fn emit_invariant_violation(&self, trigger_id: &EventId, feature: &str, reason: &str, context: serde_json::Value) {
        if let Some(emitter) = self.ctx.emitter.as_ref() {
            emitter.emit_child(
                RuntimeEvent::InvariantDiscovered(canon_event::InvariantDiscovered {
                    feature: feature.to_string(),
                    confidence: 1.0,
                    support: 1,
                }),
                vec![trigger_id.clone()],
                file!(),
                line!(),
            );
        }
        self.emit_error(
            trigger_id,
            "plan_invariant_violation",
            format!("plan invariant violated: {reason}"),
            "warning",
            serde_json::json!({
                "feature": feature,
                "reason": reason,
                "context": context,
            }),
        );
    }

    fn should_reject_verifier_sequence(
        &self,
        event: &RuntimeEvent,
    ) -> Option<(&'static str, String, serde_json::Value)> {
        let step = match event {
            RuntimeEvent::LoopVerified(_) => Some(MetaInvariantVerifierSequenceStep::LoopVerified),
            RuntimeEvent::VerifierPolicyUpdated(_) => {
                Some(MetaInvariantVerifierSequenceStep::VerifierPolicyUpdated)
            }
            RuntimeEvent::LoopRewarded(_) => Some(MetaInvariantVerifierSequenceStep::LoopRewarded),
            _ => None,
        }?;
        let Some(reason) = meta_invariant_verifier_sequence_contract(
            step,
            self.ctx.last_control_kind.as_deref(),
            self.ctx.pending_required_successor.as_deref(),
            self.ctx.last_verified.is_some(),
        ) else {
            return None;
        };
        Some((
            "meta_invariant_verifier_sequence_contract",
            reason.to_string(),
            serde_json::json!({
                "event_kind": canon_event::event_kind_str(event),
                "sequence_step": step.as_str(),
                "last_control_kind": self.ctx.last_control_kind,
                "pending_required_successor": self.ctx.pending_required_successor,
                "has_last_verified": self.ctx.last_verified.is_some(),
            }),
        ))
    }

    fn should_reject_planned_action(&self, planned: &canon_event::LoopPlanned) -> Option<(&'static str, String, serde_json::Value)> {
        let semantic_summary = self
            .ctx
            .last_observed
            .as_ref()
            .map(|observed| &observed.semantic_summary)
            .cloned()
            .unwrap_or_default();
        let action_class = meta_invariant_classify_planned_action_class(
            planned.action_kind.as_str(),
            &planned.action_payload,
        );
        if meta_invariant_no_progress_forces_change(
            self.ctx.objective_trend_state.current_no_progress_streak,
            action_class,
        )
        {
            return Some((
                "meta_invariant_no_progress_forces_change",
                "stalled loop forbids passive discovery or verification as the next first action".to_string(),
                serde_json::json!({
                    "action_kind": planned.action_kind,
                    "action_class": action_class.as_str(),
                    "no_progress_streak": self.ctx.objective_trend_state.current_no_progress_streak,
                }),
            ));
        }
        let actionable_failure = meta_invariant_has_actionable_failure(
            semantic_summary.validation_blocked_by_preconditions,
            semantic_summary.compiler_repair_required,
            semantic_summary.planning_preconditions.len(),
            semantic_summary.compiler_hints.len(),
            semantic_summary.module_gaps.len(),
        );
        let constraint_action = classify_constraint_action(planned);
        let target_root = semantic_summary
            .target_root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.ctx.workspace.clone());
        let constraint_decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState {
                semantic_path_exists: semantic_summary.path_exists,
                semantic_cargo_project: semantic_summary.cargo_project,
                real_path_exists: target_root.exists(),
                real_cargo_project: target_root.join("Cargo.toml").exists(),
                actionable_failure,
                validation_blocked: semantic_summary.validation_blocked_by_preconditions,
                entrypoint_missing: matches!(semantic_summary.entrypoint_kind.as_deref(), Some("none") | None)
                    && semantic_summary.cargo_project,
                module_gaps_present: !semantic_summary.module_gaps.is_empty(),
                recent_no_semantic_progress: false,
                failure_class_no_actionable: semantic_summary.primary_failure_class().as_deref() == Some("no_actionable_failure"),
                failure_scope_localized: semantic_summary.failure_scope.as_deref() == Some("localized"),
                failure_scope_workspace: semantic_summary.failure_scope.as_deref() == Some("workspace"),
                failure_scope_tooling: semantic_summary.failure_scope.as_deref() == Some("tooling"),
                route_objective_contradiction: false,
            },
            route: None,
            action: constraint_action,
            deterministic_route: None,
        });
        if let ConstraintDecision::Forbid(reason) = constraint_decision {
            return Some((
                "meta_invariant_shared_constraint_forbid",
                reason.to_string(),
                serde_json::json!({
                    "action_kind": planned.action_kind,
                    "constraint_action": constraint_action.map(|a| format!("{a:?}")),
                    "compiler_repair_required": semantic_summary.compiler_repair_required,
                    "compiler_hints": semantic_summary.compiler_hints.len(),
                    "planning_preconditions": semantic_summary.planning_preconditions.len(),
                    "entrypoint_kind": semantic_summary.entrypoint_kind,
                    "module_gaps": semantic_summary.module_gaps,
                }),
            ));
        }
        if !meta_invariant_any_action_cites_failure(
            &planned.action_payload,
            semantic_summary.primary_failure_class().as_deref(),
        ) {
            return Some((
                "meta_invariant_plan_must_cite_failure",
                "planned action does not cite the active failure_class".to_string(),
                serde_json::json!({
                    "action_kind": planned.action_kind,
                    "active_failure_class": semantic_summary.primary_failure_class(),
                    "cited_failure_class": planned.action_payload.get("failure_class").and_then(|v| v.as_str()),
                }),
            ));
        }
        if !meta_invariant_action_must_declare_verifier(
            planned.action_kind.as_str(),
            &planned.action_payload,
        ) {
            return Some((
                "meta_invariant_action_must_declare_verifier",
                "mutating planned actions must declare the expected verifier".to_string(),
                serde_json::json!({
                    "action_kind": planned.action_kind,
                    "expected_verifier": meta_invariant_expected_verifier(
                        planned.action_kind.as_str(),
                        &planned.action_payload,
                    ),
                }),
            ));
        }
        None
    }

    fn emit_stage_result(&self, trigger_id: &EventId, result: LoopStageResult) {
        let Some(emitter) = self.ctx.emitter.as_ref() else {
            return;
        };
        match result {
            LoopStageResult::Emit(event) => {
                emitter.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
            }
            LoopStageResult::EmitMany(events) => {
                for event in events {
                    emitter.emit_with_parents(event, vec![trigger_id.clone()], file!(), line!());
                }
            }
            LoopStageResult::Deferred | LoopStageResult::Noop => {}
        }
    }

    fn execute_reward_recovery(&mut self, trigger_id: &EventId, trigger_event: &RuntimeEvent) -> EventOutcome {
        let Some(last_verified) = self.ctx.last_verified.clone() else {
            return EventOutcome::NoOp("reward_recovery_missing_context");
        };
        match crate::stage::reward::execute(last_verified, &mut self.ctx) {
            Ok(LoopStageResult::Deferred) => {
                let eval = evaluate_recovery_execution(RecoveryOperation::RewardRecovery, StageExecutionOutcomeClass::Deferred);
                if let (Some(kind), Some(reason)) = (eval.debug_kind, eval.debug_reason) {
                    self.emit_debug(trigger_id, kind, reason, serde_json::json!({}));
                }
            }
            Ok(LoopStageResult::Noop) => {
                let eval = evaluate_recovery_execution(RecoveryOperation::RewardRecovery, StageExecutionOutcomeClass::Noop);
                if let (Some(kind), Some(reason)) = (eval.debug_kind, eval.debug_reason) {
                    self.emit_debug(trigger_id, kind, reason, serde_json::json!({}));
                }
            }
            Ok(LoopStageResult::Emit(event)) => self.emit_stage_result(trigger_id, LoopStageResult::Emit(event)),
            Ok(LoopStageResult::EmitMany(events)) => self.emit_stage_result(trigger_id, LoopStageResult::EmitMany(events)),
            Err(err) => {
                let eval = evaluate_recovery_execution(RecoveryOperation::RewardRecovery, StageExecutionOutcomeClass::Error);
                self.emit_error(
                    trigger_id,
                    eval.error_kind.unwrap_or("reward_recovery_execution"),
                    err.to_string(),
                    "error",
                    serde_json::json!({ "event": canon_event::event_kind_str(trigger_event) }),
                )
            }
        }
        EventOutcome::NoOp("reward_recovery_executed")
    }

    fn execute_observe_mode(
        &mut self,
        trigger_id: &EventId,
        trigger_event: &RuntimeEvent,
        mode: ObserveExecutionMode,
    ) {
        let operation = match mode {
            ObserveExecutionMode::Forced => RecoveryOperation::ObserveForced,
            ObserveExecutionMode::Triggered => RecoveryOperation::ObserveTriggered,
            _ => return,
        };
        let result = match mode {
            ObserveExecutionMode::Forced => observe::execute_forced(&mut self.ctx),
            ObserveExecutionMode::Triggered => observe::execute(&mut self.ctx),
            _ => return,
        };
        match result {
            Ok(LoopStageResult::Deferred) => {
                let eval = evaluate_recovery_execution(operation, StageExecutionOutcomeClass::Deferred);
                if let (Some(kind), Some(reason)) = (eval.debug_kind, eval.debug_reason) {
                    self.emit_debug(
                        trigger_id,
                        kind,
                        reason,
                        serde_json::json!({ "trigger_kind": canon_event::event_kind_str(trigger_event) }),
                    );
                }
            }
            Ok(LoopStageResult::Noop) => {
                let eval = evaluate_recovery_execution(operation, StageExecutionOutcomeClass::Noop);
                if let (Some(kind), Some(reason)) = (eval.debug_kind, eval.debug_reason) {
                    self.emit_debug(
                        trigger_id,
                        kind,
                        reason,
                        serde_json::json!({ "trigger_kind": canon_event::event_kind_str(trigger_event) }),
                    );
                }
            }
            Ok(result) => self.emit_stage_result(trigger_id, result),
            Err(err) => {
                let eval = evaluate_recovery_execution(operation, StageExecutionOutcomeClass::Error);
                self.emit_error(
                    trigger_id,
                    eval.error_kind.unwrap_or("observe_stage_execution"),
                    err.to_string(),
                    "error",
                    serde_json::json!({ "event": canon_event::event_kind_str(trigger_event) }),
                );
            }
        }
    }
}

fn classify_constraint_action(planned: &canon_event::LoopPlanned) -> Option<ConstraintAction> {
    match planned.action_kind.as_str() {
        "run_command" => planned
            .action_payload
            .get("cmd")
            .and_then(|v| v.as_str())
            .map(|cmd| {
                if cmd.contains("cargo init") {
                    ConstraintAction::CargoInit
                } else if cmd.contains("cargo new") {
                    ConstraintAction::CargoNew
                } else if cmd.contains("cargo check") || cmd.contains("cargo build") || cmd.contains("cargo test") {
                    ConstraintAction::Validation
                } else {
                    ConstraintAction::RepairWorkspace
                }
            }),
        "apply_patch"
        | "patch_file"
        | "write_file"
        | "edit.rename_symbol"
        | "edit.move_symbol"
        | "edit.add_import"
        | "edit.define_symbol_stub"
        | "edit.create_module_file" => Some(ConstraintAction::RepairLocalized),
        _ => None,
    }
}

impl EventConsumer for LoopStageExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn consumer_name(&self) -> &'static str {
        "loop_stage_executor"
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.ctx.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        if let Some((feature, reason, context)) = self.should_reject_verifier_sequence(event) {
            self.emit_invariant_violation(&trigger_id, feature, &reason, context);
            return EventOutcome::NoOp("verifier_sequence_invariant_violation");
        }
        let recovery_eval = if let RuntimeEvent::Debug(debug) = event {
            if debug.kind == "recovery_event" {
                let expected = debug
                    .payload
                    .get("context")
                    .and_then(|v| v.get("expected_successor"))
                    .and_then(|v| v.as_str());
                Some((expected, evaluate_recovery_event(
                    expected,
                    self.ctx.pending_required_successor.as_deref(),
                    self.ctx.last_verified.is_some(),
                )))
            } else {
                None
            }
        } else {
            None
        };
        if let RuntimeEvent::Debug(debug) = event {
            if debug.kind == "recovery_event" {
                let expected = recovery_eval
                    .as_ref()
                    .and_then(|(expected, _)| *expected)
                    .unwrap_or("unknown");
                self.emit_debug(
                    &trigger_id,
                    "recovery_received",
                    "recovery event received by loop stage executor",
                    serde_json::json!({
                        "expected_successor": expected,
                        "trigger_kind": canon_event::event_kind_str(event),
                    }),
                );

                if let Some((_, eval)) = recovery_eval.as_ref() {
                    if eval.execute_reward_recovery || matches!(eval.rule, RecoveryEventRule::SkipRewardAlreadySatisfied | RecoveryEventRule::MissingRewardContext) {
                        if matches!(eval.rule, RecoveryEventRule::SkipRewardAlreadySatisfied) {
                            self.emit_debug(
                                &trigger_id,
                                "reward_recovery_skipped_already_satisfied",
                                "reward recovery skipped because loop_rewarded is no longer the pending successor",
                                serde_json::json!({
                                    "pending_required_successor": self.ctx.pending_required_successor,
                                    "last_control_kind": self.ctx.last_control_kind,
                                    "last_control_event_id": self.ctx.last_control_event_id,
                                }),
                            );
                            return EventOutcome::NoOp("reward_recovery_already_satisfied");
                        }
                        if matches!(eval.rule, RecoveryEventRule::MissingRewardContext) {
                            self.emit_debug(
                                &trigger_id,
                                "reward_recovery_missing_context",
                                "reward recovery requested but no last verified event is available",
                                serde_json::json!({
                                    "trigger_kind": canon_event::event_kind_str(event),
                                }),
                            );
                            self.emit_error(
                                &trigger_id,
                                "reward_stall",
                                "reward recovery requested without last_verified context".to_string(),
                                "warning",
                                serde_json::json!({ "recoverable": true }),
                            );
                            return EventOutcome::NoOp("reward_recovery_missing_context");
                        }
                    }
                    if eval.execute_reward_recovery {
                        return self.execute_reward_recovery(&trigger_id, event);
                    }
                }
            }
        }
        self.consume_control_successor(event);

        let mut trigger_observe = false;
        let force_observe_recovery = recovery_eval
            .as_ref()
            .is_some_and(|(_, eval)| eval.force_observe_recovery);
        match event {
            RuntimeEvent::Debug(debug) if debug.kind == "recovery_event" => {}
            RuntimeEvent::Debug(debug) if debug.kind == "route_objective_contradiction" => {
                self.ctx.objective_trend_state.record_route_objective_contradiction();
            }
            RuntimeEvent::Debug(debug) if debug.kind == "goal_objective_drift" => {
                self.ctx.objective_trend_state.record_goal_objective_drift();
            }
            RuntimeEvent::Debug(debug) if debug.kind == "semantic_graph_proof_verified" => {
                let artifact_id = debug
                    .payload
                    .get("payload")
                    .and_then(|v| v.get("artifact_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let result = SemanticExecutionResultRecord::new(
                    "graph_proof_verified",
                    format!("semantic graph proof verified against artifact {artifact_id}"),
                    Vec::new(),
                    true,
                );
                self.ctx.objective_trend_state.record_execution_results(std::slice::from_ref(&result));
                self.ctx.recent_execution_results.push(result);
                if self.ctx.recent_execution_results.len() > 16 {
                    let drop_n = self.ctx.recent_execution_results.len() - 16;
                    self.ctx.recent_execution_results.drain(0..drop_n);
                }
            }
            RuntimeEvent::Debug(debug) if debug.kind == "semantic_graph_proof_failed" => {
                let artifact_id = debug
                    .payload
                    .get("payload")
                    .and_then(|v| v.get("artifact_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let result = SemanticExecutionResultRecord::new(
                    "graph_proof_failed",
                    format!("semantic graph proof failed against artifact {artifact_id}"),
                    Vec::new(),
                    false,
                );
                self.ctx.objective_trend_state.record_execution_results(std::slice::from_ref(&result));
                self.ctx.recent_execution_results.push(result);
                if self.ctx.recent_execution_results.len() > 16 {
                    let drop_n = self.ctx.recent_execution_results.len() - 16;
                    self.ctx.recent_execution_results.drain(0..drop_n);
                }
            }
            RuntimeEvent::RouteSelected(rs) => {
                self.ctx.last_route_rationale = Some(rs.rationale.clone());
                self.ctx.last_route_confidence = rs.confidence.map(|c| c as f64);
                if !rs.rationale.is_empty() {
                    self.ctx.last_route_rationale_non_empty = Some(rs.rationale.clone());
                    self.ctx.last_route_confidence_non_empty = rs.confidence.map(|c| c as f64);
                }
            }
            RuntimeEvent::Tick(Tick { tick, .. }) => {
                self.ctx.current_tick = *tick;
            }
            RuntimeEvent::AgentRegistered(AgentRegistered { payload }) => {
                if let Some(id) = payload.get("agent_id").and_then(|v| v.as_str()) {
                    let cap = payload.get("capacity").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    self.ctx.scheduler.agent_capacity.insert(id.to_string(), cap);
                }
            }
            RuntimeEvent::RustcGraphArtifactWritten(written) => {
                if let Some(observed) = self.ctx.last_observed.as_mut() {
                    observed.semantic_summary.apply_graph_artifact_summary(
                        written.artifact_id.clone(),
                        written.node_count as usize,
                        written.edge_count as usize,
                        written.file_count as usize,
                        written.call_edge_count as usize,
                        written.module_edge_count as usize,
                        written.cfg_edge_count as usize,
                    );
                    self.ctx
                        .objective_trend_state
                        .record_observation(observed.error_count, &observed.semantic_summary);
                }
            }
            RuntimeEvent::RustcCaptureCompleted(_) => {}
            RuntimeEvent::RustcCaptureFailed(failed) => {
                if let Some(observed) = self.ctx.last_observed.as_mut() {
                    observed.semantic_summary.apply_rustc_capture_failure(&failed.message);
                    self.ctx
                        .objective_trend_state
                        .record_observation(observed.error_count, &observed.semantic_summary);
                }
                self.ctx.error_count = self.ctx.error_count.saturating_add(1);
                self.ctx.recent_compiler_errors.push(serde_json::json!({
                    "reason": "rustc_capture_failed",
                    "message": {
                        "level": "error",
                        "message": failed.message,
                    }
                }));
                if self.ctx.recent_compiler_errors.len() > 16 {
                    let drop_n = self.ctx.recent_compiler_errors.len() - 16;
                    self.ctx.recent_compiler_errors.drain(0..drop_n);
                }
                trigger_observe = true;
            }
            RuntimeEvent::LoopObserved(o) => {
                self.ctx.last_observed = Some(o.clone());
                self.ctx.last_observed_tick = Some(o.tick);
                self.ctx.errors_before = o.error_count;
                self.ctx
                    .objective_trend_state
                    .record_observation(o.error_count, &o.semantic_summary);
            }
            RuntimeEvent::LoopActed(a) => {
                let action_outcome = classify_action_outcome(&a.action_kind, a.success, &a.stdout, &a.stderr);
                let bootstrap_eval = evaluate_bootstrap_effects(action_outcome);
                self.ctx.last_acted = Some(a.clone());
                self.ctx.last_action_kind = a.action_kind.clone();
                self.ctx.batch_acted.push(a.clone());
                self.ctx.last_planned_observed_tick = None;
                self.ctx.last_handled_observed_hash = None;
                self.ctx.last_emitted_plan_hash = None;
                self.ctx.last_delta_hash = None;
                const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files", "done"];
                if !READ_ONLY_ACTIONS.contains(&a.action_kind.as_str()) {
                    self.ctx.consecutive_invalid_plan_batches = 0;
                    self.ctx.last_invalid_plan_reason = None;
                    self.ctx.last_invalid_plan_planned_count = None;
                }
                if let Some(action_id) = a.action_id.clone() {
                    if let Some(intents) = self.ctx.action_semantic_intents.remove(&action_id) {
                        let results = execution_results_for_action(&intents, a.success, &a.stderr);
                        self.ctx.objective_trend_state.record_execution_results(&results);
                        self.ctx.recent_execution_results.extend(results);
                        if self.ctx.recent_execution_results.len() > 16 {
                            let drop_n = self.ctx.recent_execution_results.len() - 16;
                            self.ctx.recent_execution_results.drain(0..drop_n);
                        }
                    }
                    if let Some(paths) = self.ctx.write_paths_by_action.remove(&action_id) {
                        for p in paths {
                            self.ctx.file_write_tracker.release(&p);
                        }
                    }
                    if !READ_ONLY_ACTIONS.contains(&a.action_kind.as_str()) {
                        self.ctx.dirty_tracker.mark_dirty("orchestrator", Some(&action_id));
                    }
                    for task in self.ctx.dep_tracker.complete(&action_id) {
                        self.ctx.scheduler.push(task);
                    }
                }
                for task in self.ctx.dep_tracker.complete(&a.action_kind) {
                    self.ctx.scheduler.push(task);
                }
                if bootstrap_eval.clear_scheduler {
                    self.ctx.scheduler.clear();
                }
                if bootstrap_eval.clear_dep_tracker {
                    self.ctx.dep_tracker.clear();
                }
                if bootstrap_eval.clear_pending_act {
                    self.ctx.pending_act = None;
                }
                if bootstrap_eval.clear_active_batch {
                    self.ctx.active_batch_llm_request_id = None;
                }
                if bootstrap_eval.clear_act_batch_tracker {
                    self.ctx.act_batch_tracker.clear();
                }
                if bootstrap_eval.emit_refresh_required {
                    self.emit_debug(
                        &trigger_id,
                        "bootstrap_refresh_required",
                        "successful bootstrap invalidated queued plan work; forcing a fresh observe is required",
                        serde_json::json!({
                            "action_kind": a.action_kind,
                            "success": a.success,
                            "target_workspace": self.ctx
                                .goal_text
                                .as_deref()
                                .and_then(|t| canon_goal::parse_agent_goal_markdown(t).target_path)
                                .map(|p| p.display().to_string()),
                        }),
                    );
                }
            }
            RuntimeEvent::VerifierPolicyUpdated(updated) => {
                self.ctx.last_verifier_outcome = Some(updated.verifier_outcome.clone());
                self.ctx.last_verifier_retry_policy = Some(updated.retry_policy.clone());
                self.ctx.last_verifier_reward_bias = Some(updated.reward_bias.clone());
                self.ctx.last_verifier_actionable_failure = Some(updated.actionable_failure);
                let result = SemanticExecutionResultRecord::new(
                    match updated.retry_policy.as_str() {
                        "corrective_retry" => "verifier_policy_corrective_retry",
                        _ => "verifier_policy_none",
                    },
                    format!(
                        "meta_invariant_all_results_update_policy verifier_outcome={} retry_policy={} reward_bias={} actionable_failure={}",
                        updated.verifier_outcome,
                        updated.retry_policy,
                        updated.reward_bias,
                        updated.actionable_failure
                    ),
                    Vec::new(),
                    !updated.actionable_failure,
                )
                .with_attempted_kind("verify_result");
                self.ctx
                    .objective_trend_state
                    .record_execution_results(std::slice::from_ref(&result));
                self.ctx.recent_execution_results.push(result);
                if self.ctx.recent_execution_results.len() > 16 {
                    let drop_n = self.ctx.recent_execution_results.len() - 16;
                    self.ctx.recent_execution_results.drain(0..drop_n);
                }
            }
            RuntimeEvent::LoopVerified(v) => {
                self.ctx.last_verified = Some(v.clone());
                if v.passed {
                    self.ctx.error_count = 0;
                    self.ctx.warning_count = 0;
                    self.ctx.dirty_tracker.mark_verified("orchestrator");
                }
                self.ctx.last_handled_observed_hash = None;
                self.ctx.last_planned_observed_tick = None;
                self.ctx.last_emitted_plan_hash = None;
                self.ctx.last_delta_hash = None;
            }
            RuntimeEvent::SubTaskResult(r) => {
                self.ctx.context_merger.absorb(r, &r.agent_id);
            }
            RuntimeEvent::LoopPlanned(p) => {
                if let Some((feature, reason, context)) = self.should_reject_planned_action(p) {
                    self.emit_invariant_violation(&trigger_id, feature, &reason, context);
                    return EventOutcome::NoOp("plan_invariant_violation");
                }
                if let Some(action_id) = p.action_id.as_ref() {
                    let target_root = self
                        .ctx
                        .last_observed
                        .as_ref()
                        .and_then(|observed| observed.semantic_summary.target_root.as_deref())
                        .map(std::path::Path::new);
                    let intents =
                        classify_planned_action_intents(&p.action_kind, &p.action_payload, target_root);
                    self.ctx.action_semantic_intents.insert(action_id.clone(), intents);
                }
                let priority = infer_priority(p, self.ctx.goodness, self.ctx.delta_g);
                let task = ScheduledTask { priority, enqueued_at: Instant::now(), seq: 0, agent_id: None, plan: p.clone() };
                if !p.depends_on.is_empty() {
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        if let Some(child) = p.action_id.as_ref() {
                            for dep in &p.depends_on {
                                emitter.emit_child(RuntimeEvent::GoalEdgeDefined(GoalEdgeDefined { from_node_id: dep.clone(), to_node_id: child.clone(), created: true }), vec![trigger_id.clone()], file!(), line!());
                            }
                        }
                    }
                    self.ctx.dep_tracker.add(task);
                } else {
                    self.ctx.scheduler.push(task);
                }
            }
            RuntimeEvent::LoopRewarded(r) => {
                if r.halt {
                    self.ctx.halted = true;
                    let reason = if self.ctx.last_action_kind == "conclude" {
                        "explicit conclude route".to_string()
                    } else {
                        format!(
                            "loop_rewarded requested halt; reward={} stagnant_ticks={} errors_before={} errors_after={}",
                            r.reward, r.stagnant_ticks, r.errors_before, r.errors_after
                        )
                    };
                    self.ctx.last_halt_reason = Some(reason.clone());
                    if let Some(emitter) = self.ctx.emitter.as_ref() {
                        emitter.emit_child(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "loop_stage_executor".to_string(),
                                kind: "loop_halt_reason".to_string(),
                                payload: decision_trace_payload(
                                    "loop entered halted state",
                                    serde_json::json!({
                                        "reason": reason,
                                        "tick": r.tick,
                                        "reward": r.reward,
                                        "stagnant_ticks": r.stagnant_ticks,
                                        "errors_before": r.errors_before,
                                        "errors_after": r.errors_after,
                                    }),
                                ),
                            }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
            }
            RuntimeEvent::PlanningCompleted(pc) => {
                self.ctx.objective_trend_state.record_planning_completion(&pc.status);
                let transition =
                    evaluate_loop_transition(self.ctx.pending_required_successor.as_deref(), Some(&pc.status), None, None);
                if transition
                    .recovery_rules
                    .contains(&LoopRecoveryRule::ClearPlannerSuppressionOnInvalidPlan)
                {
                    self.ctx.last_planned_observed_tick = None;
                    self.ctx.last_handled_observed_hash = None;
                    self.ctx.last_emitted_plan_hash = None;
                    self.ctx.last_delta_hash = None;
                } else {
                    self.ctx.consecutive_invalid_plan_batches = 0;
                    self.ctx.last_invalid_plan_reason = None;
                    self.ctx.last_invalid_plan_planned_count = None;
                }
            }
            RuntimeEvent::GoodnessSnapshot(g) => {
                self.ctx.goodness = Some(g.g);
                self.ctx.delta_g = Some(g.delta_g);
                self.ctx.objective_trend_state.record_goodness(g.g, g.delta_g);
            }
            RuntimeEvent::RuntimeStateUpdated(updated) => {
                if updated.payload.get("fatal_invariant").and_then(|v| v.as_bool()).unwrap_or(false) {
                    self.ctx.halted = true;
                    self.ctx.last_halt_reason = updated
                        .payload
                        .get("fatal_invariant_reason")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string());
                } else if updated.payload.get("runtime_mode").and_then(|v| v.as_str()) == Some("running") {
                    self.ctx.halted = false;
                    self.ctx.last_halt_reason = None;
                }
            }
            RuntimeEvent::PromptLoaded(prompt) => {
                let data = prompt.payload.get("data").unwrap_or(&prompt.payload);
                if let Some(content) = data.get("content").and_then(|c| c.as_str()) {
                    self.ctx.goal_text = Some(content.to_string());
                    self.ctx.last_prompted_goal = Some(content.to_string());
                    trigger_observe = true;
                }
            }
            RuntimeEvent::ErrorOccurred(err) => {
                if err.severity == "warning" {
                    self.ctx.warning_count = self.ctx.warning_count.saturating_add(1);
                } else {
                    self.ctx.error_count = self.ctx.error_count.saturating_add(1);
                }
                self.ctx.recent_compiler_errors.push(serde_json::json!({
                    "reason": "error_occurred",
                    "message": {
                        "level": err.severity,
                        "message": err.message,
                    }
                }));
                if self.ctx.recent_compiler_errors.len() > 16 {
                    let drop_n = self.ctx.recent_compiler_errors.len() - 16;
                    self.ctx.recent_compiler_errors.drain(0..drop_n);
                }
                if err.kind == "invalid_plan_batch" {
                    self.ctx.consecutive_invalid_plan_batches =
                        self.ctx.consecutive_invalid_plan_batches.saturating_add(1);
                    self.ctx.objective_trend_state.record_invalid_plan_event();
                    self.ctx.last_invalid_plan_reason = Some(err.message.clone());
                    self.ctx.last_invalid_plan_planned_count = err
                        .context
                        .get("planned_count")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize);
                }
                let fatal_invariant_diag = err.kind == "diagnostics_triggered"
                    && err
                        .context
                        .get("fatal_invariant")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                let transition =
                    evaluate_loop_transition(self.ctx.pending_required_successor.as_deref(), None, Some(&err.kind), None);
                let explicit_observe_recovery = transition.trigger_observe;
                if !matches!(
                    evaluate_error_observe(&err.kind, explicit_observe_recovery, fatal_invariant_diag),
                    ErrorObserveRule::None
                ) {
                    trigger_observe = true;
                }
            }
            RuntimeEvent::ToolResult(r) if r.kind != "llm.plan" => {
                self.ctx.batch_tool_results.push(r.clone());
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_)
            | RuntimeEvent::RustcCaptureStarted(_) => {}
        }
        self.consume_control_successor(event);
        self.record_control_state(event, &trigger_id);

        let suppress_observe_on_invariant =
            matches!(event, RuntimeEvent::ErrorOccurred(err) if err.kind == "invariant_violation")
                || matches!(event, RuntimeEvent::Code(code) if matches!(code.delta.event, canon_event::RustcEvent::InvariantViolation(_)));

        let runtime_eval = evaluate_loop_runtime(
            self.ctx.halted,
            force_observe_recovery,
            trigger_observe,
            suppress_observe_on_invariant,
            self.ctx.pending_required_successor.as_deref(),
            matches!(event, RuntimeEvent::RouteSelected(_)),
        );

        if runtime_eval.observe_mode == ObserveExecutionMode::Forced {
            self.execute_observe_mode(&trigger_id, event, ObserveExecutionMode::Forced);
        } else if runtime_eval.observe_mode == ObserveExecutionMode::SuppressedByPendingSuccessor {
            self.emit_debug(
                &trigger_id,
                "observe_suppressed_due_to_pending_successor",
                "observe suppressed because another control successor is required",
                serde_json::json!({
                    "pending_required_successor": self.ctx.pending_required_successor,
                    "last_control_kind": self.ctx.last_control_kind,
                    "last_control_event_id": self.ctx.last_control_event_id,
                    "trigger_kind": canon_event::event_kind_str(event),
                }),
            );
        } else if runtime_eval.observe_mode == ObserveExecutionMode::Triggered {
            self.execute_observe_mode(&trigger_id, event, ObserveExecutionMode::Triggered);
        }

        if runtime_eval.halt_blocks_stage {
            if runtime_eval.warn_route_selected_while_halted {
                self.emit_debug(
                    &trigger_id,
                    "loop_stage_blocked",
                    "loop stage execution blocked because context is halted",
                    serde_json::json!({
                        "event_kind": canon_event::event_kind_str(event),
                        "last_halt_reason": self.ctx.last_halt_reason,
                    }),
                );
                self.emit_error(
                    &trigger_id,
                    "loop_stage_halted",
                    "route_selected received while loop context is halted".to_string(),
                    "warning",
                    serde_json::json!({
                        "event_kind": canon_event::event_kind_str(event),
                        "last_halt_reason": self.ctx.last_halt_reason,
                        "recoverable": true,
                    }),
                );
            }
            return EventOutcome::NoOp("loop_stage_halted");
        }

        let Ok(stage) = LoopStageEvent::try_from(event.clone()) else {
            return EventOutcome::NoOp("loop_stage_not_stage_event");
        };
        let res = stage.execute(&mut self.ctx, trigger_id.clone());
        if self.ctx.emitter.is_none() {
            return EventOutcome::NoOp("loop_stage_no_emitter");
        }
        match res {
            Ok(result) => self.emit_stage_result(&trigger_id, result),
            Err(err) => self.emit_error(
                &trigger_id,
                "loop_stage_execution",
                err.to_string(),
                "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
            ),
        }
        EventOutcome::NoOp("loop_stage_async")
    }
}

#[cfg(test)]
mod tests {
    use super::LoopStageExecutor;
    use canon_event::{
        events::VerifierPolicyUpdated, EventConsumer, EventId, EventOutcome, LoopRewarded,
        RouteSelected, RuntimeEvent,
    };
    use std::path::PathBuf;

    fn route_selected(route: &str) -> RuntimeEvent {
        RuntimeEvent::RouteSelected(RouteSelected {
            tick: 0,
            suggested_route: route.to_string(),
            prompt: String::new(),
            approved_route: route.to_string(),
            rationale: String::new(),
            confidence: None,
            gate_note: String::new(),
            gate_rules_fired: Vec::new(),
            gate_changed: false,
            gate_should_stop: false,
            model_json: String::new(),
        })
    }

    fn loop_rewarded() -> RuntimeEvent {
        RuntimeEvent::LoopRewarded(LoopRewarded {
            tick: 0,
            errors_before: 0,
            errors_after: 0,
            stagnant_ticks: 0,
            halt: false,
            goodness: 0.0,
            reward: 0.0,
            delta_g: 0.0,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    fn verifier_policy_updated() -> RuntimeEvent {
        RuntimeEvent::VerifierPolicyUpdated(VerifierPolicyUpdated {
            tick: 0,
            verifier_outcome: "passed".to_string(),
            retry_policy: "none".to_string(),
            reward_bias: "positive".to_string(),
            actionable_failure: false,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    #[test]
    fn rejects_loop_rewarded_before_verifier_policy_update() {
        let mut executor =
            LoopStageExecutor::new(PathBuf::from("/tmp/workspace"), PathBuf::from("/tmp/test.tlog"));
        let _ = executor.on_event(&route_selected("verify"), EventId::new("route-selected"));
        let outcome = executor.on_event(&loop_rewarded(), EventId::new("loop-rewarded"));
        assert!(matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }

    #[test]
    fn accepts_loop_rewarded_for_direct_conclude_route() {
        let mut executor =
            LoopStageExecutor::new(PathBuf::from("/tmp/workspace"), PathBuf::from("/tmp/test.tlog"));
        let _ = executor.on_event(&route_selected("conclude"), EventId::new("route-selected"));
        let outcome = executor.on_event(&loop_rewarded(), EventId::new("loop-rewarded"));
        assert!(!matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }

    #[test]
    fn rejects_verifier_policy_updated_before_loop_verified() {
        let mut executor =
            LoopStageExecutor::new(PathBuf::from("/tmp/workspace"), PathBuf::from("/tmp/test.tlog"));
        let _ = executor.on_event(&route_selected("verify"), EventId::new("route-selected"));
        let outcome =
            executor.on_event(&verifier_policy_updated(), EventId::new("verifier-policy-updated"));
        assert!(matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }
}
