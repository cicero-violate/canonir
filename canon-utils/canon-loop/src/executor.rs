use crate::stage::observe;
use crate::{
    context::LoopContext,
    harness_repair::{evaluate_harness_repair_loop, HarnessRepairDecision},
    policy::{
        classify_action_outcome, evaluate_bootstrap_effects, evaluate_error_observe, evaluate_loop_runtime, evaluate_loop_transition, evaluate_recovery_event, evaluate_recovery_execution,
        ErrorObserveRule, LoopRecoveryRule, ObserveExecutionMode, RecoveryEventRule, RecoveryOperation, StageExecutionOutcomeClass,
    },
    result::LoopStageResult,
    scheduler::{infer_priority, ScheduledTask},
    stage::LoopStageEvent,
};
use canon_event::events::VerifierPolicyUpdated;
use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, GoalEdgeDefined, RuntimeEvent, Tick};
use canon_invariant::decision_trace_payload;
use canon_proc_macros::must_emit;
use canon_semantic_state::{classify_planned_action_intents, execution_results_for_action, SemanticExecutionResultRecord};
use std::path::PathBuf;
use std::time::Instant;

pub struct LoopStageExecutor {
    ctx: LoopContext,
}

impl LoopStageExecutor {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - LoopStageExecutor::new", file!(), line!(), module_path!());
        Self { ctx: LoopContext::new(workspace, tlog_path) }
    }

    pub fn with_agent_id(mut self, id: String) -> Self {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - LoopStageExecutor::with_agent_id", file!(), line!(), module_path!());
        self.ctx.agent_id = Some(id);
        self
    }

    pub fn evaluate_harness_repair(&self) -> HarnessRepairDecision {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - LoopStageExecutor::evaluate_harness_repair", file!(), line!(), module_path!());
        evaluate_harness_repair_loop(&self.ctx.harness_repair_state())
    }

    pub fn evaluate_harness_repair_for_target(&mut self, target: &crate::harness_repair::HarnessRepairTarget, failure_output: &str) -> crate::harness_repair::HarnessRepairDirective {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - LoopStageExecutor::evaluate_harness_repair_for_target", file!(), line!(), module_path!());
        self.ctx.prime_harness_repair_target(target, failure_output);
        crate::harness_repair::build_harness_repair_directive(&self.ctx.harness_repair_state(), target)
    }

    fn consume_control_successor(&mut self, event: &RuntimeEvent) {
        let _event_kind = canon_event::event_kind_str(event);
        // removed: successor consumption (handled by invariants)
    }
    fn emit_debug(&mut self, trigger_id: &EventId, kind: &str, reason: &str, payload: serde_json::Value) {
        if let Some(emitter) = self.ctx.emitter.as_ref() {
            let debug_payload = decision_trace_payload(reason, payload);
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            debug_payload.hash(&mut hasher);
            let debug_hash = hasher.finish();

            if self.ctx.last_delta_hash.as_ref() != Some(&debug_hash) {
                self.ctx.last_delta_hash = Some(debug_hash);
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent { source: "loop_stage_executor".to_string(), kind: kind.to_string(), payload: debug_payload }),
                    vec![trigger_id.clone()],
                    file!(),
                    line!(),
                );
            }
        }
    }

    fn emit_error(&self, trigger_id: &EventId, kind: &str, message: String, severity: &str, context: serde_json::Value) {
        if let Some(emitter) = self.ctx.emitter.as_ref() {
            emitter.emit_child(RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(kind, "loop_stage_executor", message, severity, context, None)), vec![trigger_id.clone()], file!(), line!());
        }
    }

    // removed: should_reject_verifier_sequence (invariant logic moved to validation layer)

    // removed: should_reject_planned_action (validation layer responsibility)

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

    fn execute_observe_mode(&mut self, trigger_id: &EventId, trigger_event: &RuntimeEvent, mode: ObserveExecutionMode) {
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
                    self.emit_debug(trigger_id, kind, reason, serde_json::json!({ "trigger_kind": canon_event::event_kind_str(trigger_event) }));
                }
            }
            Ok(LoopStageResult::Noop) => {
                let eval = evaluate_recovery_execution(operation, StageExecutionOutcomeClass::Noop);
                if let (Some(kind), Some(reason)) = (eval.debug_kind, eval.debug_reason) {
                    self.emit_debug(trigger_id, kind, reason, serde_json::json!({ "trigger_kind": canon_event::event_kind_str(trigger_event) }));
                }
            }
            Ok(result) => self.emit_stage_result(trigger_id, result),
            Err(err) => {
                let eval = evaluate_recovery_execution(operation, StageExecutionOutcomeClass::Error);
                self.emit_error(trigger_id, eval.error_kind.unwrap_or("observe_stage_execution"), err.to_string(), "error", serde_json::json!({ "event": canon_event::event_kind_str(trigger_event) }));
            }
        }
    }

    fn record_execution_result(&mut self, result: SemanticExecutionResultRecord) {
        self.ctx.objective_trend_state.record_execution_results(std::slice::from_ref(&result));
        self.ctx.recent_execution_results.push(result);
        if self.ctx.recent_execution_results.len() > 16 {
            let drop_n = self.ctx.recent_execution_results.len() - 16;
            self.ctx.recent_execution_results.drain(0..drop_n);
        }
    }

    fn record_graph_proof_result(&mut self, debug: &canon_event::DebugEvent, passed: bool) {
        let artifact_id = debug.payload.get("payload").and_then(|v| v.get("artifact_id")).and_then(|v| v.as_str()).unwrap_or("unknown");
        let result = SemanticExecutionResultRecord::new(
            if passed { "graph_proof_verified" } else { "graph_proof_failed" },
            if passed { format!("semantic graph proof verified against artifact {artifact_id}") } else { format!("semantic graph proof failed against artifact {artifact_id}") },
            Vec::new(),
            passed,
        );
        self.record_execution_result(result);
    }

    fn apply_verifier_policy_update(&mut self, updated: &VerifierPolicyUpdated) {
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
                updated.verifier_outcome, updated.retry_policy, updated.reward_bias, updated.actionable_failure
            ),
            Vec::new(),
            !updated.actionable_failure,
        )
        .with_attempted_kind("verify_result");
        self.record_execution_result(result);
    }

    fn record_compiler_error(&mut self, reason: &str, level: &str, message: &str) {
        self.ctx.recent_compiler_errors.push(serde_json::json!({
            "reason": reason,
            "message": {
                "level": level,
                "message": message,
            }
        }));
        if self.ctx.recent_compiler_errors.len() > 16 {
            let drop_n = self.ctx.recent_compiler_errors.len() - 16;
            self.ctx.recent_compiler_errors.drain(0..drop_n);
        }
    }

    fn reset_plan_window_state(&mut self) {
        self.ctx.last_planned_observed_tick = None;
        self.ctx.last_handled_observed_hash = None;
        self.ctx.last_emitted_plan_hash = None;
        self.ctx.last_delta_hash = None;
    }

    fn clear_invalid_plan_tracking(&mut self) {
        self.ctx.consecutive_invalid_plan_batches = 0;
        self.ctx.last_invalid_plan_reason = None;
        self.ctx.last_invalid_plan_planned_count = None;
    }

    fn handle_route_selected(&mut self, selected: &canon_event::RouteSelected) {
        self.ctx.last_route_rationale = Some(selected.rationale.clone());
        self.ctx.last_route_confidence = selected.confidence.map(|c| c as f64);
        if !selected.rationale.is_empty() {
            self.ctx.last_route_rationale_non_empty = Some(selected.rationale.clone());
            self.ctx.last_route_confidence_non_empty = selected.confidence.map(|c| c as f64);
        }
    }

    fn handle_loop_verified(&mut self, verified: &canon_event::LoopVerified) {
        self.ctx.last_verified = Some(verified.clone());
        if verified.passed {
            self.ctx.error_count = 0;
            self.ctx.warning_count = 0;
            self.ctx.dirty_tracker.mark_verified("orchestrator");
        }
        self.reset_plan_window_state();
    }

    fn handle_planning_completed(&mut self, completed: &canon_event::PlanningCompleted) {
        self.ctx.objective_trend_state.record_planning_completion(&completed.status);
        self.apply_planning_transition_effects(Some(&completed.status), None);
    }

    fn handle_runtime_state_updated(&mut self, updated: &canon_event::RuntimeStateUpdated) {
        if updated.payload.get("fatal_invariant").and_then(|v| v.as_bool()).unwrap_or(false) {
            self.ctx.halted = true;
            self.ctx.last_halt_reason = updated.payload.get("fatal_invariant_reason").and_then(|v| v.as_str()).map(|v| v.to_string());
        } else if updated.payload.get("runtime_mode").and_then(|v| v.as_str()) == Some("running") {
            self.ctx.halted = false;
            self.ctx.last_halt_reason = None;
        }
    }

    fn handle_prompt_loaded(&mut self, prompt: &canon_event::PromptLoaded) -> bool {
        let data = prompt.payload.get("data").unwrap_or(&prompt.payload);
        let Some(content) = data.get("content").and_then(|c| c.as_str()) else {
            return false;
        };
        self.ctx.goal_text = Some(content.to_string());
        self.ctx.last_prompted_goal = Some(content.to_string());
        true
    }

    fn handle_loop_planned(&mut self, trigger_id: &EventId, planned: &canon_event::LoopPlanned) -> Option<EventOutcome> {
        // removed: planned-action invariant rejection (handled in validation layer)

        if let Some(action_id) = planned.action_id.as_ref() {
            let target_root = self.ctx.last_observed.as_ref().and_then(|observed| observed.semantic_summary.target_root.as_deref()).map(std::path::Path::new);
            let intents = classify_planned_action_intents(&planned.action_kind, &planned.action_payload, target_root);
            self.ctx.action_semantic_intents.insert(action_id.clone(), intents);
        }

        let priority = infer_priority(planned, self.ctx.goodness, self.ctx.delta_g);
        let task = ScheduledTask { priority, enqueued_at: Instant::now(), seq: 0, agent_id: None, plan: planned.clone() };

        if !planned.depends_on.is_empty() {
            if let Some(emitter) = self.ctx.emitter.as_ref() {
                if let Some(child) = planned.action_id.as_ref() {
                    for dep in &planned.depends_on {
                        emitter.emit_child(
                            RuntimeEvent::GoalEdgeDefined(GoalEdgeDefined { from_node_id: dep.clone(), to_node_id: child.clone(), created: true }),
                            vec![trigger_id.clone()],
                            file!(),
                            line!(),
                        );
                    }
                }
            }
            self.ctx.dep_tracker.add(task);
        } else {
            self.ctx.scheduler.push(task);
        }

        None
    }

    fn handle_rustc_capture_failed(&mut self, failed: &canon_event::RustcCaptureFailed) -> bool {
        if let Some(observed) = self.ctx.last_observed.as_mut() {
            observed.semantic_summary.apply_rustc_capture_failure(&failed.message);
            self.ctx.objective_trend_state.record_observation(observed.error_count, &observed.semantic_summary);
        }
        self.ctx.error_count = self.ctx.error_count.saturating_add(1);
        self.record_compiler_error("rustc_capture_failed", "error", &failed.message);
        true
    }

    fn handle_loop_observed(&mut self, observed: &canon_event::LoopObserved) {
        // FIX: deduplicate consecutive LoopObserved with same tick
        if self.ctx.last_observed_tick == Some(observed.tick) {
            return;
        }
        self.ctx.last_observed = Some(observed.clone());
        self.ctx.last_observed_tick = Some(observed.tick);
        self.ctx.errors_before = observed.error_count;
        self.ctx.objective_trend_state.record_observation(observed.error_count, &observed.semantic_summary);
    }

    fn handle_agent_registered(&mut self, payload: &serde_json::Value) {
        if let Some(id) = payload.get("agent_id").and_then(|v| v.as_str()) {
            let cap = payload.get("capacity").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            self.ctx.scheduler.agent_capacity.insert(id.to_string(), cap);
        }
    }

    fn handle_rustc_graph_artifact_written(&mut self, written: &canon_event::RustcGraphArtifactWritten) {
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
            self.ctx.objective_trend_state.record_observation(observed.error_count, &observed.semantic_summary);
        }
    }

    fn handle_goodness_snapshot(&mut self, snapshot: &canon_event::GoodnessSnapshot) {
        self.ctx.goodness = Some(snapshot.g);
        self.ctx.delta_g = Some(snapshot.delta_g);
        self.ctx.objective_trend_state.record_goodness(snapshot.g, snapshot.delta_g);
    }

    fn handle_subtask_result(&mut self, result: &canon_event::SubTaskResult) {
        self.ctx.context_merger.absorb(result, &result.agent_id);
    }

    fn handle_tool_result(&mut self, result: &canon_event::ToolResult) {
        if result.kind != "llm.plan" {
            self.ctx.batch_tool_results.push(result.clone());
        }
    }

    fn handle_objective_trend_debug(&mut self, kind: &str) -> bool {
        match kind {
            "route_objective_contradiction" => {
                self.ctx.objective_trend_state.record_route_objective_contradiction();
                true
            }
            "goal_objective_drift" => {
                self.ctx.objective_trend_state.record_goal_objective_drift();
                true
            }
            _ => false,
        }
    }

    fn handle_loop_rewarded(&mut self, trigger_id: &EventId, rewarded: &canon_event::LoopRewarded) {
        if !rewarded.halt {
            return;
        }
        self.ctx.halted = true;
        let reason = if self.ctx.last_action_kind == "conclude" {
            "explicit conclude route".to_string()
        } else {
            format!(
                "loop_rewarded requested halt; reward={} stagnant_ticks={} errors_before={} errors_after={}",
                rewarded.reward, rewarded.stagnant_ticks, rewarded.errors_before, rewarded.errors_after
            )
        };
        self.ctx.last_halt_reason = Some(reason.clone());
        self.emit_debug(
            trigger_id,
            "loop_halt_reason",
            "loop entered halted state",
            serde_json::json!({
                "reason": reason,
                "tick": rewarded.tick,
                "reward": rewarded.reward,
                "stagnant_ticks": rewarded.stagnant_ticks,
                "errors_before": rewarded.errors_before,
                "errors_after": rewarded.errors_after,
            }),
        );
    }

    fn handle_loop_acted(&mut self, trigger_id: &EventId, acted: &canon_event::LoopActed) {
        let action_outcome = classify_action_outcome(&acted.action_kind, acted.success, &acted.stdout, &acted.stderr);
        let bootstrap_eval = evaluate_bootstrap_effects(action_outcome);
        self.ctx.last_acted = Some(acted.clone());
        self.ctx.last_action_kind = acted.action_kind.clone();
        self.ctx.batch_acted.push(acted.clone());
        self.apply_post_act_cleanup(&acted.action_kind);

        if let Some(action_id) = acted.action_id.clone() {
            if let Some(intents) = self.ctx.action_semantic_intents.remove(&action_id) {
                let results = execution_results_for_action(&intents, acted.success, &acted.stderr);
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
            if !matches!(acted.action_kind.as_str(), "list_dir" | "read_file" | "search_files" | "done") {
                self.ctx.dirty_tracker.mark_dirty("orchestrator", Some(&action_id));
            }
            for task in self.ctx.dep_tracker.complete(&action_id) {
                self.ctx.scheduler.push(task);
            }
        }

        for task in self.ctx.dep_tracker.complete(&acted.action_kind) {
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
                trigger_id,
                "bootstrap_refresh_required",
                "successful bootstrap invalidated queued plan work; forcing a fresh observe is required",
                serde_json::json!({
                    "action_kind": acted.action_kind,
                    "success": acted.success,
                    "target_workspace": self.ctx
                        .goal_text
                        .as_deref()
                        .and_then(|t| canon_goal::parse_agent_goal_markdown(t).target_path)
                        .map(|p| p.display().to_string()),
                }),
            );
        }
    }

    fn apply_post_act_cleanup(&mut self, action_kind: &str) {
        self.reset_plan_window_state();

        const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files", "done"];
        if !READ_ONLY_ACTIONS.contains(&action_kind) {
            self.clear_invalid_plan_tracking();
        }
    }

    fn handle_error_occurred(&mut self, err: &canon_event::ErrorOccurred, trigger_observe: &mut bool) {
        if err.severity == "warning" {
            self.ctx.warning_count = self.ctx.warning_count.saturating_add(1);
        } else {
            self.ctx.error_count = self.ctx.error_count.saturating_add(1);
        }
        self.record_compiler_error("error_occurred", &err.severity, &err.message);

        if err.kind == "invalid_plan_batch" {
            self.ctx.consecutive_invalid_plan_batches = self.ctx.consecutive_invalid_plan_batches.saturating_add(1);
            self.ctx.objective_trend_state.record_invalid_plan_event();
            self.ctx.last_invalid_plan_reason = Some(err.message.clone());
            self.ctx.last_invalid_plan_planned_count = err.context.get("planned_count").and_then(|v| v.as_u64()).map(|v| v as usize);
        }

        let fatal_invariant_diag = err.kind == "diagnostics_triggered" && err.context.get("fatal_invariant").and_then(|v| v.as_bool()).unwrap_or(false);

        let transition = self.apply_planning_transition_effects(None, Some(&err.kind));
        if transition {
            *trigger_observe = true;
        }

        *trigger_observe |= self.should_trigger_observe_from_error(&err.kind, fatal_invariant_diag);
    }

    fn should_trigger_observe_from_error(&self, error_kind: &str, fatal_invariant: bool) -> bool {
        matches!(evaluate_error_observe(error_kind, false, fatal_invariant), ErrorObserveRule::GenericErrorObserve)
    }

    fn apply_planning_transition_effects(&mut self, planning_status: Option<&str>, error_kind: Option<&str>) -> bool {
        let transition = evaluate_loop_transition(self.ctx.pending_required_successor.as_deref(), planning_status, error_kind, None);

        if transition.recovery_rules.contains(&LoopRecoveryRule::ClearPlannerSuppressionOnInvalidPlan) {
            self.reset_plan_window_state();
        } else if planning_status.is_some() {
            self.clear_invalid_plan_tracking();
        }

        transition.recovery_rules.contains(&LoopRecoveryRule::TriggerObserveOnActStall)
    }

    fn handle_recovery_event(&mut self, event: &RuntimeEvent, trigger_id: &EventId, recovery_eval: &Option<(Option<String>, crate::policy::RecoveryEventEvaluation)>) -> Option<EventOutcome> {
        let RuntimeEvent::Debug(debug) = event else {
            return None;
        };
        if debug.kind != "recovery_event" {
            return None;
        }

        let expected = recovery_eval.as_ref().and_then(|(expected, _)| expected.as_deref()).unwrap_or("unknown");
        self.emit_debug(
            trigger_id,
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
                        trigger_id,
                        "reward_recovery_skipped_already_satisfied",
                        "reward recovery skipped because loop_rewarded is no longer the pending successor",
                        serde_json::json!({
                            "pending_required_successor": self.ctx.pending_required_successor,
                            "last_control_kind": self.ctx.last_control_kind,
                            "last_control_event_id": self.ctx.last_control_event_id,
                        }),
                    );
                    return Some(EventOutcome::NoOp("reward_recovery_already_satisfied"));
                }

                if matches!(eval.rule, RecoveryEventRule::MissingRewardContext) {
                    self.emit_debug(
                        trigger_id,
                        "reward_recovery_missing_context",
                        "reward recovery requested but no last verified event is available",
                        serde_json::json!({
                            "trigger_kind": canon_event::event_kind_str(event),
                        }),
                    );
                    self.emit_error(trigger_id, "reward_stall", "reward recovery requested without last_verified context".to_string(), "warning", serde_json::json!({ "recoverable": true }));
                    return Some(EventOutcome::NoOp("reward_recovery_missing_context"));
                }
            }

            if eval.execute_reward_recovery {
                return Some(self.execute_reward_recovery(trigger_id, event));
            }
        }

        None
    }

    fn handle_runtime_observe_mode(&mut self, trigger_id: &EventId, event: &RuntimeEvent, observe_mode: ObserveExecutionMode) {
        if observe_mode == ObserveExecutionMode::Forced {
            self.execute_observe_mode(trigger_id, event, ObserveExecutionMode::Forced);
        } else if observe_mode == ObserveExecutionMode::SuppressedByPendingSuccessor {
            self.emit_debug(
                trigger_id,
                // FIX: remove suppression reason; Observe must not be blocked by pending successor
                "observe_emitted_unconditionally",
                "observe suppressed because another control successor is required",
                serde_json::json!({
                    "pending_required_successor": self.ctx.pending_required_successor,
                    "last_control_kind": self.ctx.last_control_kind,
                    "last_control_event_id": self.ctx.last_control_event_id,
                    "trigger_kind": canon_event::event_kind_str(event),
                }),
            );
        } else if observe_mode == ObserveExecutionMode::Triggered {
            self.execute_observe_mode(trigger_id, event, ObserveExecutionMode::Triggered);
        }
    }

    fn suppresses_observe_on_invariant(event: &RuntimeEvent) -> bool {
        matches!(event, RuntimeEvent::ErrorOccurred(err) if err.kind == "invariant_violation")
            || matches!(event, RuntimeEvent::Code(code) if matches!(code.delta.event, canon_event::RustcEvent::InvariantViolation(_)))
    }

    fn apply_runtime_evaluation(&mut self, trigger_id: &EventId, event: &RuntimeEvent, force_observe_recovery: bool, trigger_observe: bool) -> Option<EventOutcome> {
        let suppress_observe_on_invariant = Self::suppresses_observe_on_invariant(event);

        // FIX: fail-safe — clear stuck pending successor if it remains after any subsequent event
        if self.ctx.pending_required_successor.as_deref() == Some("route_selected") {
            self.ctx.pending_required_successor = None;
        }

        // FIX: ignore pending_required_successor entirely (it is stuck and causing infinite suppression)
        let runtime_eval = evaluate_loop_runtime(
            self.ctx.halted,
            force_observe_recovery,
            trigger_observe,
            suppress_observe_on_invariant,
            None,
            matches!(event, RuntimeEvent::RouteSelected(_)),
        );

        // FIX: clear pending_required_successor when RouteSelected is observed
        if matches!(event, RuntimeEvent::RouteSelected(_)) {
            self.ctx.pending_required_successor = None;
        }

        self.handle_runtime_observe_mode(trigger_id, event, runtime_eval.observe_mode);

        if runtime_eval.halt_blocks_stage {
            if runtime_eval.warn_route_selected_while_halted {
                self.emit_debug(
                    trigger_id,
                    "loop_stage_blocked",
                    "loop stage execution blocked because context is halted",
                    serde_json::json!({
                        "event_kind": canon_event::event_kind_str(event),
                        "last_halt_reason": self.ctx.last_halt_reason,
                    }),
                );
                self.emit_error(
                    trigger_id,
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
            return Some(EventOutcome::NoOp("loop_stage_halted"));
        }

        None
    }

    fn build_recovery_eval(event: &RuntimeEvent, pending_required_successor: Option<&str>, has_last_verified: bool) -> Option<(Option<String>, crate::policy::RecoveryEventEvaluation)> {
        let RuntimeEvent::Debug(debug) = event else {
            return None;
        };
        if debug.kind != "recovery_event" {
            return None;
        }
        let expected = debug.payload.get("context").and_then(|v| v.get("expected_successor")).and_then(|v| v.as_str()).map(str::to_string);
        Some((expected.clone(), evaluate_recovery_event(expected.as_deref(), pending_required_successor, has_last_verified)))
    }

    fn recovery_forces_observe(recovery_eval: &Option<(Option<String>, crate::policy::RecoveryEventEvaluation)>) -> bool {
        recovery_eval.as_ref().is_some_and(|(_, eval)| eval.force_observe_recovery)
    }

    fn advance_control_state(&mut self, event: &RuntimeEvent, trigger_id: &EventId) {
        self.consume_control_successor(event);
        let next = match event {
            RuntimeEvent::RouteSelected(rs) => match rs.approved_route.to_ascii_lowercase().as_str() {
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

    fn execute_stage_event(&mut self, trigger_id: &EventId, event: &RuntimeEvent) -> EventOutcome {
        let Ok(stage) = LoopStageEvent::try_from(event.clone()) else {
            return EventOutcome::NoOp("loop_stage_not_stage_event");
        };
        let res = stage.execute(&mut self.ctx, trigger_id.clone());
        if self.ctx.emitter.is_none() {
            return EventOutcome::NoOp("loop_stage_no_emitter");
        }
        match res {
            Ok(result) => self.emit_stage_result(trigger_id, result),
            Err(err) => self.emit_error(trigger_id, "loop_stage_execution", err.to_string(), "error", serde_json::json!({ "event": format!("{:?}", event) })),
        }
        EventOutcome::NoOp("loop_stage_async")
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
        let recovery_eval = Self::build_recovery_eval(event, self.ctx.pending_required_successor.as_deref(), self.ctx.last_verified.is_some());
        if let Some(outcome) = self.handle_recovery_event(event, &trigger_id, &recovery_eval) {
            return outcome;
        }
        let mut trigger_observe = false;
        let force_observe_recovery = Self::recovery_forces_observe(&recovery_eval);
        match event {
            RuntimeEvent::Debug(debug) if debug.kind == "recovery_event" => {}
            RuntimeEvent::Debug(debug) if self.handle_objective_trend_debug(debug.kind.as_str()) => {}
            RuntimeEvent::Debug(debug) if debug.kind == "semantic_graph_proof_verified" => {
                self.record_graph_proof_result(debug, true);
            }
            RuntimeEvent::Debug(debug) if debug.kind == "semantic_graph_proof_failed" => {
                self.record_graph_proof_result(debug, false);
            }
            RuntimeEvent::RouteSelected(rs) => {
                self.handle_route_selected(rs);
            }
            RuntimeEvent::Tick(Tick { tick, .. }) => {
                self.ctx.current_tick = *tick;
            }
            RuntimeEvent::AgentRegistered(AgentRegistered { payload }) => {
                self.handle_agent_registered(payload);
            }
            RuntimeEvent::RustcGraphArtifactWritten(written) => {
                self.handle_rustc_graph_artifact_written(written);
            }
            RuntimeEvent::RustcCaptureCompleted(_) => {}
            RuntimeEvent::RustcCaptureFailed(failed) => {
                trigger_observe |= self.handle_rustc_capture_failed(failed);
            }
            RuntimeEvent::LoopObserved(o) => {
                self.handle_loop_observed(o);
            }
            RuntimeEvent::LoopActed(a) => {
                self.handle_loop_acted(&trigger_id, a);
            }
            RuntimeEvent::VerifierPolicyUpdated(updated) => {
                self.apply_verifier_policy_update(updated);
            }
            RuntimeEvent::LoopVerified(v) => {
                self.handle_loop_verified(v);
            }
            RuntimeEvent::SubTaskResult(r) => {
                self.handle_subtask_result(r);
            }
            RuntimeEvent::LoopPlanned(p) => {
                if let Some(outcome) = self.handle_loop_planned(&trigger_id, p) {
                    return outcome;
                }
            }
            RuntimeEvent::LoopRewarded(r) => {
                self.handle_loop_rewarded(&trigger_id, r);
            }
            RuntimeEvent::PlanningCompleted(pc) => {
                self.handle_planning_completed(pc);
            }
            RuntimeEvent::GoodnessSnapshot(g) => {
                self.handle_goodness_snapshot(g);
            }
            RuntimeEvent::RuntimeStateUpdated(updated) => {
                self.handle_runtime_state_updated(updated);
            }
            RuntimeEvent::PromptLoaded(prompt) => {
                trigger_observe |= self.handle_prompt_loaded(prompt);
            }
            RuntimeEvent::ErrorOccurred(err) => {
                self.handle_error_occurred(err, &mut trigger_observe);
            }
            RuntimeEvent::ToolResult(r) if r.kind != "llm.plan" => {
                self.handle_tool_result(r);
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_) => {
                // IGNORE: RequestDispatch deprecated
                return EventOutcome::NoOp("request_dispatch_ignored");
            }
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
        self.advance_control_state(event, &trigger_id);

        if let Some(outcome) = self.apply_runtime_evaluation(&trigger_id, event, force_observe_recovery, trigger_observe) {
            return outcome;
        }

        self.execute_stage_event(&trigger_id, event)
    }
}
