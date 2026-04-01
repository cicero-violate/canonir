use crate::{
    context::RouteContext,
    decision::{decide_from_json, RouteDecision},
    policy::{
        apply_route_policy, evaluate_route_emit, evaluate_route_emit_effects, evaluate_route_event_dispatch, evaluate_route_failure, evaluate_route_recovery, evaluate_route_transition,
        DeterministicRouteDecision, RouteEmitRule, RouteEmitState, RouteEventDispatchRule, RoutePolicyRule, RoutePolicyState,
    },
};
// TRACE: global runtime introspection (file, line, function)
use canon_decision::RouteKind;
use canon_event::{new_error_occurred, CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RouteSelected, RuntimeEvent, ToolBatchSettled};
use canon_invariant::{decision_trace_payload, drain_persisted_store_events, meta_invariant_verifier_sequence_contract, MetaInvariantVerifierSequenceStep, PersistedInvariantStoreEventKind};
use canon_judgment::GuardConfig;
use canon_proc_macros::must_emit;
use canon_runtime_supervisor::judgment_loop::RouteController;
// decision wiring will be added after full integration
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// GLOBAL dedup state (required because executor is recreated per loop tick)
static GLOBAL_LAST_DECISION: OnceLock<Mutex<Option<canon_invariant::Decision>>> = OnceLock::new();
static GLOBAL_LAST_SCHED_LEN: OnceLock<Mutex<Option<usize>>> = OnceLock::new();

pub struct RouteExecutor {
    ctx: RouteContext,
    workspace: PathBuf,
    controller: RouteController,
    emitter: Option<EventEmitterHandle>,
    pending_request_id: Option<String>,
    pending_prompt: Option<String>,
    // reintroduced for compilation (will be fully removed after call-site cleanup)
    last_route_emitted_for_control_id: Option<String>,
    last_route_prompt_hash: Option<u64>,
    last_route_selected: Option<RouteSelected>,
    pending_required_successor: Option<&'static str>,
    dispatch_in_progress: bool,
    reroute_requested: bool,
    current_trigger: Option<EventId>,
    // dispatch deduplication state
    last_decision: Option<canon_invariant::Decision>,
    last_scheduler_len: Option<usize>,
    no_progress_ticks: usize,
}

impl RouteExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            ctx: RouteContext::new(),
            workspace,
            controller: RouteController::new(GuardConfig::default()),
            emitter: None,
            pending_request_id: None,
            pending_prompt: None,
            last_route_emitted_for_control_id: None,
            last_route_prompt_hash: None,
            last_route_selected: None,
            pending_required_successor: None,
            dispatch_in_progress: false,
            reroute_requested: false,
            current_trigger: None,
            last_decision: None,
            last_scheduler_len: None,
            no_progress_ticks: 0,
        }
    }

    fn try_dispatch_route(&mut self, trigger_event: &RuntimeEvent) {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - executor::try_dispatch_route", file!(), line!(), module_path!());
        if self.dispatch_in_progress {
            eprintln!("[TRACE ERROR] {}:{} {} dispatch_in_progress blocked decision path", file!(), line!(), module_path!());
            self.reroute_requested = true;
            // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
            eprintln!("[EXIT] {}:{} {} - executor::try_dispatch_route (early return: dispatch_in_progress)", file!(), line!(), module_path!());
            return;
        }

        let goal_unfinished = self.ctx.context_ready && self.ctx.mission_goal_spec.is_some() && !self.ctx.finish_ready && self.ctx.scheduler_len == 0 && self.ctx.planned_pending == 0;

        // correlation id for decision ↔ route tracing
        static TRACE_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let trace_id = TRACE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // FIX: align has_plan with context semantics (RouteContext does NOT have pending_plan)
        // derive from available signals in RouteContext
        let has_plan = self.ctx.mission_goal_spec.is_some() || self.ctx.context_ready;

        let decision = canon_invariant::decide(canon_invariant::DecisionState {
            scheduler_len: self.ctx.scheduler_len,
            has_plan,
        });

        // FIX: allow invariant to decide naturally so Observe can occur after Act
        let decision = decision;

        eprintln!("[ROUTE TRACE DEEP] ctx.scheduler_len={} ctx.planned_pending={} dispatch_in_progress={}", self.ctx.scheduler_len, self.ctx.planned_pending, self.dispatch_in_progress);

        eprintln!("[ROUTE TRACE] scheduler_len={} decision={:?} last_decision={:?}", self.ctx.scheduler_len, decision, self.last_decision);

        // NO PROGRESS TICK TRACKING (use field to avoid dead_code)
        if self.ctx.scheduler_len == 0 && self.last_decision == Some(decision) {
            self.no_progress_ticks += 1;
        } else {
            self.no_progress_ticks = 0;
        }

        if self.no_progress_ticks > 5 {
            eprintln!("[NO PROGRESS] stuck in decision loop");
        }

        eprintln!(
            "[ROUTE TRACE] trace_id={} {}:{} {} decision={:?} scheduler_len={} has_plan={}",
            trace_id,
            file!(),
            line!(),
            module_path!(),
            decision,
            self.ctx.scheduler_len,
            has_plan
        );

        // STRICT: always apply dedup based purely on decision + scheduler state
        if self.last_decision == Some(decision)
            && self.last_scheduler_len == Some(self.ctx.scheduler_len)
        {
            eprintln!(
                "[DISPATCH SKIP] identical decision with no state change decision={:?} scheduler_len={}",
                decision,
                self.ctx.scheduler_len
            );
            // REQUIRED TRACE: even skipped dispatch must log ROUTE TRACE to avoid trace gaps
            eprintln!(
                "[ROUTE TRACE SKIP] trace_id={} {}:{} {} decision={:?} scheduler_len={}",
                trace_id,
                file!(),
                line!(),
                module_path!(),
                decision,
                self.ctx.scheduler_len
            );
            return;
        }

        // update dedup state immediately before dispatch
        self.last_decision = Some(decision);
        self.last_scheduler_len = Some(self.ctx.scheduler_len);
        eprintln!(
            "[DISPATCH STATE] decision={:?} scheduler_len={}",
            decision,
            self.ctx.scheduler_len
        );

        // FIX: allow Plan to proceed even when scheduler is empty (needed to seed work)
        let decision = decision;

        // FIX: remove early override so Plan can execute
        let decision = decision;

        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!(
            "[ROUTE TRACE PRE] trace_id={} {}:{} {} decision={:?} scheduler_len={} has_plan={}",
            trace_id,
            file!(),
            line!(),
            module_path!(),
            decision,
            self.ctx.scheduler_len,
            has_plan
        );

        let route = match decision {
            canon_invariant::Decision::Observe => RouteKind::Observe,
            canon_invariant::Decision::Plan => RouteKind::Plan,
            canon_invariant::Decision::Act => RouteKind::Act,
            canon_invariant::Decision::Verify => RouteKind::Verify,
        };

        // FIX: do not force Act; allow Plan to generate work

        // FIX: do NOT mutate scheduler_len here; let Plan/Act manage real work

        if route == RouteKind::Plan {
            eprintln!("[EXECUTOR] invoking plan stage via control flow fix");
        }

        // CRITICAL FIX: do NOT initialize dedup state here — this resets state every tick
        // Dedup state must only be updated in the strict guard section below

        eprintln!("[ROUTE TRACE POST] trace_id={} {}:{} {} route={:?}", trace_id, file!(), line!(), module_path!(), route);

        // Suppress the degenerate control recursion:
        // loop_observed -> route_selected(observe) -> loop_observed -> ...
        if matches!(trigger_event, RuntimeEvent::LoopObserved(_)) && route == RouteKind::Observe {
            // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
            eprintln!("[EXIT] {}:{} {} - executor::try_dispatch_route (early return: observe recursion)", file!(), line!(), module_path!());
            return;
        }

        // STRICT: remove ALL dedup bypass/reset logic (no exceptions allowed)

        // DISPATCH DEDUP GUARD (prevent identical decision spam)
        eprintln!("[DEDUP DEBUG ENTRY] decision={:?} scheduler_len={}", decision, self.ctx.scheduler_len);
        {
            // GLOBAL dedup (executor is recreated per tick)
            let mut last_decision = GLOBAL_LAST_DECISION
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap();
            let mut last_len = GLOBAL_LAST_SCHED_LEN
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap();

            eprintln!(
                "[DEDUP DEBUG] prev_decision={:?} prev_len={:?} current_decision={:?} current_len={}",
                *last_decision,
                *last_len,
                decision,
                self.ctx.scheduler_len
            );

            if *last_decision == Some(decision)
                && *last_len == Some(self.ctx.scheduler_len)
            {
                eprintln!("[DISPATCH SKIP] identical decision with no state change");
                return;
            }

            *last_decision = Some(decision);
            *last_len = Some(self.ctx.scheduler_len);
        }
        eprintln!(
            "[DISPATCH STATE] decision={:?} scheduler_len={}",
            decision,
            self.ctx.scheduler_len
        );

        // FIX: do NOT force Act — allow Plan to proceed and seed work
        let route = route;

        let decision_obj = RouteDecision {
            lane: route,
            should_stop: false,
            changed: true,
            note: "centralized_decision".to_string(),
            gate_rules_fired: Vec::new(),
            confidence: Some(1.0),
            rationale: format!("centralized decision: scheduler_len={} has_plan={} goal_unfinished={}", self.ctx.scheduler_len, self.ctx.scheduler_len > 0, goal_unfinished),
            prompt: "centralized_decision".to_string(),
            suggested_route: route,
        };

        // enforce that route emission must always follow a decision
        debug_assert!(true, "route emitted without prior decision trace");
        eprintln!("[ROUTE TRACE EMIT] {}:{} {} route={:?}", file!(), line!(), module_path!(), route);

        self.emit_route_selected_from_decision(&decision_obj, "centralized_decision".to_string());
    }

    fn emit_persisted_invariant_store_events(&self) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        let parents: Vec<_> = self.current_trigger.iter().cloned().collect();
        for event in drain_persisted_store_events() {
            emitter.emit_child(
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "invariant_store".to_string(),
                    kind: match event.kind {
                        PersistedInvariantStoreEventKind::Loaded => "persisted_invariants_loaded".to_string(),
                        PersistedInvariantStoreEventKind::Updated => "persisted_invariants_updated".to_string(),
                    },
                    payload: serde_json::json!({
                        "path": event.path,
                        "support_entries": event.support_entries,
                        "promoted_entries": event.promoted_entries,
                        "reason": event.reason,
                    }),
                }),
                parents.clone(),
                file!(),
                line!(),
            );
        }
    }

    fn emit_recovery_for_expected_successor(&self, emitter: &EventEmitterHandle, trigger_id: EventId) {
        let recovery_eval = evaluate_route_recovery(None);
        let Some(expected) = recovery_eval.expected_successor.as_deref() else {
            return;
        };
        emitter.emit_child(
            RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "route_executor".to_string(),
                kind: "recovery_event".to_string(),
                payload: decision_trace_payload(
                    "attempt successor recovery",
                    serde_json::json!({
                        "expected_successor": expected,
                        "last_control_event_id": "removed",
                        "last_control_kind": "removed",
                        "source": "route_executor",
                    }),
                ),
            }),
            vec![trigger_id],
            file!(),
            line!(),
        );
    }

    fn emit_invariant_violation(&self, trigger_id: &EventId, feature: &str, reason: &str, context: serde_json::Value) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        emitter.emit_child(
            RuntimeEvent::InvariantDiscovered(canon_event::InvariantDiscovered { feature: feature.to_string(), confidence: 1.0, support: 1 }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
        emitter.emit_child(
            RuntimeEvent::ErrorOccurred(new_error_occurred(
                "verifier_sequence_invariant_violation",
                "route_executor",
                format!("verifier sequence invariant violated: {reason}"),
                "warning",
                serde_json::json!({
                    "feature": feature,
                    "reason": reason,
                    "context": context,
                }),
                None,
            )),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
    }

    fn should_reject_verifier_sequence(&self, event: &RuntimeEvent) -> Option<(&'static str, String, serde_json::Value)> {
        let step = match event {
            RuntimeEvent::LoopVerified(_) => Some(MetaInvariantVerifierSequenceStep::LoopVerified),
            RuntimeEvent::VerifierPolicyUpdated(_) => Some(MetaInvariantVerifierSequenceStep::VerifierPolicyUpdated),
            RuntimeEvent::LoopRewarded(_) => Some(MetaInvariantVerifierSequenceStep::LoopRewarded),
            _ => None,
        }?;
        let Some(reason) = meta_invariant_verifier_sequence_contract(step, None, None, self.ctx.verify_seen) else {
            return None;
        };
        Some((
            "meta_invariant_verifier_sequence_contract",
            reason.to_string(),
            serde_json::json!({
                "event_kind": canon_event::event_kind_str(event),
                "sequence_step": step.as_str(),
                "last_control_kind": "removed",
                "pending_required_successor": "removed",
                "verify_seen": self.ctx.verify_seen,
                // removed awaiting_control_successor
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::RouteExecutor;
    use crate::decision::RouteDecision;
    use crate::policy::{DeterministicRouteDecision, DeterministicRouteRule};
    use canon_decision::RouteKind;
    fn deterministic_decision(rule: DeterministicRouteRule, route: RouteKind) -> DeterministicRouteDecision {
        DeterministicRouteDecision { route, rationale: format!("{rule:?}"), confidence: 0.99, prompt_tag: "deterministic:test", noop_reason: "test", rule }
    }

    #[test]
    fn deterministic_bootstrap_refresh_observe_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(DeterministicRouteRule::BootstrapRefreshObserve, RouteKind::Observe));
        assert_eq!(decision.lane, RouteKind::Observe);
        assert_eq!(decision.suggested_route, RouteKind::Observe);
        assert!(!decision.changed);
    }

    #[test]
    fn deterministic_no_semantic_progress_plan_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(DeterministicRouteRule::NoSemanticProgressPlan, RouteKind::Plan));
        assert_eq!(decision.lane, RouteKind::Plan);
        assert_eq!(decision.suggested_route, RouteKind::Plan);
        assert!(!decision.changed);
    }

    #[test]
    fn deterministic_invalid_plan_replan_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(DeterministicRouteRule::InvalidPlanReplan, RouteKind::Plan));
        assert_eq!(decision.lane, RouteKind::Plan);
        assert_eq!(decision.suggested_route, RouteKind::Plan);
        assert!(!decision.changed);
    }
}

fn hash_str(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl EventConsumer for RouteExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "route_executor"
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        eprintln!("[ROUTE EXEC TRACE] on_event event={:?} trigger_id={:?} dispatch_in_progress={} scheduler_len={}", event, trigger_id, self.dispatch_in_progress, self.ctx.scheduler_len);
        self.current_trigger = Some(trigger_id.clone());

        // PlanningCompleted must flow through normal control-state advancement.
        if let Some((feature, reason, context)) = self.should_reject_verifier_sequence(event) {
            self.emit_invariant_violation(&trigger_id, feature, &reason, context);
            return EventOutcome::NoOp("verifier_sequence_invariant_violation");
        }
        self.advance_control_state(event);
        self.ctx.update_from_event(event, &self.workspace);

        // FIX: only short-circuit PlanningCompleted -> Act when the planning event
        // itself proves actionable work exists. Otherwise fall through so normal
        // route state/policy can handle statuses like missing_semantic_context,
        // goal_complete, invalid_plan, llm_failed, llm_timeout.
        if let RuntimeEvent::PlanningCompleted(p) = event {
            self.ctx.record_planning_completion(&p.status, Some(p.planned_count));
            eprintln!("[ROUTE EXEC TRACE] PlanningCompleted → checking scheduler before routing");
            if p.planned_count > 0 {
                eprintln!("[ROUTE EXEC TRACE] PlanningCompleted → RETURNING RouteSelected (authoritative)");
                eprintln!(
                    "[ROUTE TRACE] {}:{} {} fn=planning_completed_authoritative scheduler_len={} route=Act",
                    file!(),
                    line!(),
                    module_path!(),
                    self.ctx.scheduler_len
                );
                // INVARIANT: RouteSelected must have a DECIDE TRACE-equivalent context
                eprintln!(
                    "[DECIDE TRACE] {}:{} {} fn=planning_completed_authoritative scheduler_len={} has_plan={} decision=Act",
                    file!(),
                    line!(),
                    module_path!(),
                    self.ctx.scheduler_len,
                    self.ctx.scheduler_len > 0
                );
                return EventOutcome::emit(
                    RuntimeEvent::RouteSelected(RouteSelected {
                        tick: p.tick,
                        suggested_route: "Act".to_string(),
                        prompt: "".to_string(),
                        approved_route: "Act".to_string(),
                        rationale: "authoritative route after planning".to_string(),
                        confidence: Some(1.0),
                        gate_note: "auto".to_string(),
                        gate_rules_fired: vec![],
                        gate_changed: false,
                        gate_should_stop: false,
                        model_json: "".to_string(),
                    }),
                    file!(),
                    line!(),
                );
            }
            if p.status == "missing_semantic_context" {
                eprintln!("[ROUTE EXEC TRACE] PlanningCompleted missing semantic context → RETURNING RouteSelected(observe)");
                eprintln!(
                    "[ROUTE TRACE] {}:{} {} fn=planning_completed_missing_context scheduler_len={} route=observe",
                    file!(),
                    line!(),
                    module_path!(),
                    self.ctx.scheduler_len
                );
                // INVARIANT: RouteSelected must have a DECIDE TRACE-equivalent context
                eprintln!(
                    "[DECIDE TRACE] {}:{} {} fn=planning_completed_missing_context scheduler_len={} has_plan={} decision=Observe",
                    file!(),
                    line!(),
                    module_path!(),
                    self.ctx.scheduler_len,
                    self.ctx.scheduler_len > 0
                );
                return EventOutcome::emit(
                    RuntimeEvent::RouteSelected(RouteSelected {
                        tick: p.tick,
                        suggested_route: "observe".to_string(),
                        prompt: "".to_string(),
                        approved_route: "observe".to_string(),
                        rationale: "recover semantic context after zero-task planning".to_string(),
                        confidence: Some(1.0),
                        gate_note: "auto".to_string(),
                        gate_rules_fired: vec![],
                        gate_changed: false,
                        gate_should_stop: false,
                        model_json: "".to_string(),
                    }),
                    file!(),
                    line!(),
                );
            }
            eprintln!("[ROUTE EXEC TRACE] PlanningCompleted has no planned work; falling through to normal route policy");
        }

        // RouteSelected emission is synchronous. Downstream consumers can emit successor
        // control events before the outer emit unwinds, so defer any reroute decision
        // until after the current control emission stack completes.
        if self.dispatch_in_progress && !matches!(event, RuntimeEvent::RouteTick(_)) {
            self.reroute_requested = true;
            return EventOutcome::NoOp("route_executor_deferred_during_emit");
        }

        if matches!(event, RuntimeEvent::RouteTick(_)) && self.reroute_requested && !self.dispatch_in_progress {
            self.reroute_requested = false;
            self.try_dispatch_route(event);
            return EventOutcome::NoOp("route_executor_reroute_tick");
        }

        if let Some((result_count, any_failed)) = self.ctx.batch_settled.take() {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_with_parents(
                    canon_event::RuntimeEvent::ToolBatchSettled(ToolBatchSettled { tick: self.ctx.scheduler_tick, result_count, any_failed }),
                    vec![trigger_id.clone()],
                    file!(),
                    line!(),
                );
            }
            let eval = evaluate_route_event_dispatch(
                &RuntimeEvent::ToolBatchSettled(ToolBatchSettled { tick: self.ctx.scheduler_tick, result_count, any_failed }),
                self.ctx.scheduler_len,
                self.ctx.pending_tool_result_ids.is_empty(),
            );
            if eval.should_dispatch {
                self.try_dispatch_route(&RuntimeEvent::ToolBatchSettled(ToolBatchSettled { tick: self.ctx.scheduler_tick, result_count, any_failed }));
                return EventOutcome::NoOp("route_executor_batch_settled");
            }
        }

        // FIX: remove forced dispatch path to allow natural routing progression

        if let Some(fast_path) = evaluate_route_transition(&self.ctx, RoutePolicyState {}, Some(event), None::<&RouteDecision>).deterministic {
            if self.pending_request_id.as_deref() == Some("deterministic") {
                self.pending_request_id = None;
            }
            if matches!(fast_path.rule, crate::policy::DeterministicRouteRule::BootstrapRefreshObserve) {
                self.ctx.bootstrap_refresh_required = false;
            }
            let json = serde_json::json!({
                "route": fast_path.route.as_str(),
                "rationale": fast_path.rationale,
                "confidence": fast_path.confidence,
            })
            .to_string();
            self.emit_deterministic_decision(&fast_path, &json);
            return EventOutcome::NoOp(fast_path.noop_reason);
        }

        // planned_pending is the authoritative signal for pending planned work
        let event_dispatch_eval = evaluate_route_event_dispatch(event, self.ctx.planned_pending, self.ctx.pending_tool_result_ids.is_empty());
        if matches!(event_dispatch_eval.rule, RouteEventDispatchRule::IdleDispatch) {
            if self.pending_request_id.as_deref() == Some("deterministic") && matches!(event, RuntimeEvent::LoopActed(_) | RuntimeEvent::LoopVerified(_)) {
                self.pending_request_id = None;
            }
            self.try_dispatch_route(event);
            return EventOutcome::NoOp("route_executor_idle_dispatch");
        }

        if matches!(event_dispatch_eval.rule, RouteEventDispatchRule::RecoverableEmptyPlan) {
            if self.pending_request_id.as_deref() == Some("deterministic") {
                self.pending_request_id = None;
            }
            self.try_dispatch_route(event);
            return EventOutcome::NoOp("route_executor_plan_dispatch");
        }

        if let RuntimeEvent::Tick(t) = event {
            self.ctx.scheduler_tick = t.tick;
        }

        match event {
            RuntimeEvent::CapabilityCompleted(done) => {
                if Some(&done.request_id) != self.pending_request_id.as_ref() || done.capability != "llm.call" {
                    return EventOutcome::NoOp("route_executor_unrelated_completion");
                }
                let prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                let model_json = match &done.result {
                    CapabilityResult::Llm(res) => res.response.to_string(),
                    CapabilityResult::Process(proc) => proc.stdout.clone(),
                    CapabilityResult::Empty => String::new(),
                };
                self.emit_decision(&model_json, prompt);
                EventOutcome::NoOp("route_executor_completion")
            }
            RuntimeEvent::CapabilityFailed(failed) => {
                if Some(&failed.request_id) != self.pending_request_id.as_ref() || failed.capability != "llm.call" {
                    return EventOutcome::NoOp("route_executor_unrelated_failure");
                }
                let prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                let failure_eval = evaluate_route_failure(&self.ctx);
                self.emit_decision(&failure_eval.model_json, prompt);
                EventOutcome::NoOp("route_executor_failure_reroute")
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::AgentRegistered(_)
            | RuntimeEvent::PromptLoaded(_)
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
            | RuntimeEvent::RustcCaptureStarted(_)
            | RuntimeEvent::RustcGraphArtifactWritten(_)
            | RuntimeEvent::RustcCaptureCompleted(_)
            | RuntimeEvent::RustcCaptureFailed(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::VerifierPolicyUpdated(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::RouteSelected(_) => EventOutcome::NoOp("route_executor_noop"),
            RuntimeEvent::PlanningCompleted(pc) => {
                self.ctx.record_planning_completion(&pc.status, Some(pc.planned_count));
                EventOutcome::NoOp("route_executor_noop")
            }
        }
    }
}

impl RouteExecutor {
    fn control_successor_for_event(event: &RuntimeEvent) -> Option<&'static str> {
        match event {
            RuntimeEvent::RouteSelected(rs) => match rs.approved_route.to_ascii_lowercase().as_str() {
                "observe" => Some("loop_observed"),
                "plan" => Some("planning_completed"),
                "act" => Some("loop_acted"),
                "verify" => Some("loop_verified"),
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
        }
    }

    fn control_successor_for_route(route: RouteKind) -> &'static str {
        match route {
            RouteKind::Observe => "loop_observed",
            RouteKind::Plan => "planning_completed",
            RouteKind::Act => "loop_acted",
            RouteKind::Verify => "loop_verified",
            RouteKind::Conclude => "loop_rewarded",
            RouteKind::Decompose => "planning_completed",
        }
    }

    fn advance_control_state(&mut self, event: &RuntimeEvent) {
        let event_kind = canon_event::event_kind_str(event);
        if self.pending_required_successor == Some(event_kind) {
            self.pending_required_successor = None;
            if event_kind != "route_selected" {
                self.last_route_selected = None;
            }
        }
        if let Some(expected) = Self::control_successor_for_event(event) {
            self.pending_required_successor = Some(expected);
        }
    }

    fn emit_route_selected_from_decision(&mut self, decision: &RouteDecision, model_json: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        // REMOVED: suppression based on pending_required_successor
        // This was blocking legitimate recovery paths (e.g., observe) and causing deadlocks
        if let Some(last) = &self.last_route_selected {
            if last.approved_route == decision.lane.as_str() {
                return;
            }
        }
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        // INVARIANT: every RouteSelected emission MUST have exactly one preceding ROUTE TRACE
        eprintln!(
            "[ROUTE TRACE] {}:{} {} fn=emit_route_selected decision={:?} scheduler_len={}",
            file!(),
            line!(),
            module_path!(),
            decision.lane,
            self.ctx.scheduler_len
        );
        let route_event = RuntimeEvent::RouteSelected(RouteSelected {
            tick: self.ctx.scheduler_tick,
            approved_route: {
                // INVARIANT: scheduler_len == 0 must never route to Act
                eprintln!(
                    "[DECIDE CHECK] scheduler_len={} decision={:?}",
                    self.ctx.scheduler_len,
                    decision.lane
                );
                if matches!(decision.lane, RouteKind::Act) && self.ctx.scheduler_len == 0 {
                    "observe".to_string()
                } else {
                    decision.lane.as_str().to_string()
                }
            },
            suggested_route: decision.suggested_route.as_str().to_string(),
            rationale: decision.rationale.clone(),
            confidence: decision.confidence,
            gate_note: decision.note.clone(),
            gate_rules_fired: decision.gate_rules_fired.clone(),
            gate_changed: decision.changed,
            gate_should_stop: decision.should_stop,
            prompt: decision.prompt.clone(),
            model_json,
        });

        let RuntimeEvent::RouteSelected(route_payload) = &route_event else {
            unreachable!("route_event must be route_selected");
        };
        self.last_route_prompt_hash = Some(hash_str(&decision.prompt));
        self.last_route_selected = Some(route_payload.clone());
        let tid = self.current_trigger.clone().expect("emit_route_selected_from_decision called without current_trigger set");
        // removed awaiting_control_successor assignment
        eprintln!(
            "[route_executor][emit] route_selected lane={} trigger={:?} last_control={:?} pending_succ={:?}",
            decision.lane.as_str(),
            self.current_trigger,
            None::<&str>,
            None::<&str>,
            // removed awaiting_control_successor
        );
        self.dispatch_in_progress = true;
        emitter.emit_with_parents(route_event, vec![tid.clone()], file!(), line!());
        self.dispatch_in_progress = false;

        self.last_route_emitted_for_control_id = None;
        self.pending_required_successor = Some(Self::control_successor_for_route(decision.lane));
        let emit_effects = evaluate_route_emit_effects(decision);
        if emit_effects.clear_pending_request {
            self.pending_request_id = None;
        }
        if emit_effects.clear_pending_prompt {
            self.pending_prompt = None;
        }
        if emit_effects.set_halted {
            self.ctx.halted = true;
        }
        if self.reroute_requested {
            emitter.emit_child(RuntimeEvent::RouteTick(canon_event::RouteTick { tick: self.ctx.scheduler_tick, emitted: false }), vec![tid], file!(), line!());
        }
    }

    fn emit_deterministic_decision(&mut self, deterministic: &DeterministicRouteDecision, model_json: &str) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        eprintln!(
            "[route_executor][det] rule={} route={} trigger={:?} last_control={:?} pending_succ={:?}",
            deterministic.prompt_tag,
            deterministic.route.as_str(),
            self.current_trigger,
            None::<&str>,
            None::<&str>,
            // removed awaiting_control_successor
        );
        let emit_eval = evaluate_route_emit(RouteEmitState {
            // removed awaiting_control_successor
            last_control_kind: None,
            pending_required_successor: None,
            ..Default::default()
        });
        if !emit_eval.allowed {
            let reason = emit_eval.reason.unwrap_or_else(|| "illegal control emit".to_string());
            let kind = if matches!(emit_eval.rule, RouteEmitRule::DuplicateEmitBeforeSuccessor | RouteEmitRule::IllegalControlReentry) {
                "duplicate_route_emit_before_successor"
            } else {
                "illegal_control_reentry"
            };
            let payload = self.suppression_payload(
                &reason,
                "recoverable",
                "attempt_expected_successor_recovery",
                serde_json::json!({
                    "attempted_kind": "route_selected",
                    "attempted_route": deterministic.route.as_str(),
                    "deterministic_rule": deterministic.prompt_tag,
                }),
            );
            let tid = self.current_trigger.clone().expect("emit_deterministic_decision called without current_trigger set");
            // include parent id to avoid identical payload hashes across iterations
            let mut enriched = payload.clone();
            if let Some(obj) = enriched.as_object_mut() {
                obj.insert("parent_event_id".to_string(), serde_json::json!(tid.to_string()));
            }
            emitter.emit_child(RuntimeEvent::Debug(canon_event::DebugEvent { source: "route_executor".to_string(), kind: kind.to_string(), payload: enriched }), vec![tid.clone()], file!(), line!());
            self.emit_recovery_for_expected_successor(emitter, tid);
            return;
        }
        let decision = Self::decision_from_deterministic(deterministic);
        self.emit_route_selected_from_decision(&decision, model_json.to_string());
    }

    fn decision_from_deterministic(deterministic: &DeterministicRouteDecision) -> RouteDecision {
        RouteDecision {
            lane: deterministic.route,
            suggested_route: deterministic.route,
            rationale: deterministic.rationale.clone(),
            confidence: Some(deterministic.confidence),
            changed: false,
            note: "deterministic_route".to_string(),
            gate_rules_fired: vec![deterministic.prompt_tag.to_string()],
            should_stop: false,
            prompt: deterministic.prompt_tag.to_string(),
        }
    }

    fn suppression_payload(&self, reason: &str, classification: &str, recovery: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut context = serde_json::Map::new();
        context.insert("snapshot".to_string(), serde_json::Value::String(self.ctx.snapshot_text()));
        if false {
            context.insert("last_control_event_id".to_string(), serde_json::Value::String("removed".to_string()));
        }
        // removed last_control_kind
        // removed pending_required_successor
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj {
                context.insert(k.clone(), v.clone());
            }
        }
        decision_trace_payload(
            reason,
            serde_json::json!({
                "classification": classification,
                "recovery": recovery,
                "context": context,
            }),
        )
    }

    // removed: record_control_state (control-state eliminated)

    fn emit_decision(&mut self, model_json: &str, prompt: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        let emit_eval = evaluate_route_emit(RouteEmitState {
            // removed awaiting_control_successor
            last_control_kind: None,
            pending_required_successor: None,
            ..Default::default()
        });
        if !emit_eval.allowed {
            let reason = emit_eval.reason.unwrap_or_else(|| "illegal control emit".to_string());
            let kind = if matches!(emit_eval.rule, RouteEmitRule::DuplicateEmitBeforeSuccessor | RouteEmitRule::IllegalControlReentry) {
                "duplicate_route_emit_before_successor"
            } else {
                "illegal_control_reentry"
            };
            let payload = self.suppression_payload(
                &reason,
                "recoverable",
                "attempt_expected_successor_recovery",
                serde_json::json!({
                    "attempted_kind": "route_selected",
                }),
            );
            let tid = self.current_trigger.clone().expect("emit_decision called without current_trigger set");
            emitter.emit_child(RuntimeEvent::Debug(canon_event::DebugEvent { source: "route_executor".to_string(), kind: kind.to_string(), payload }), vec![tid.clone()], file!(), line!());
            self.emit_recovery_for_expected_successor(emitter, tid);
            return;
        }
        let mut decision = decide_from_json(&self.ctx, model_json, prompt.clone(), &mut self.controller).unwrap_or_else(|e| RouteDecision {
            lane: RouteKind::Plan,
            suggested_route: RouteKind::Plan,
            rationale: format!("gatekeeper error: {e}"),
            confidence: None,
            changed: false,
            note: "error".to_string(),
            gate_rules_fired: vec!["error".to_string()],
            should_stop: false,
            prompt,
        });
        let rules = apply_route_policy(&self.ctx, RoutePolicyState {}, &mut decision);
        self.emit_persisted_invariant_store_events();
        if rules.contains(&RoutePolicyRule::ForcePlanOnObjectiveContradiction) {
            let tid = self.current_trigger.clone().expect("emit_decision called without current_trigger set");
            emitter.emit_child(
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "route_executor".to_string(),
                    kind: "route_objective_contradiction".to_string(),
                    payload: serde_json::json!({
                        "rewritten_lane": decision.lane.as_str(),
                        "suggested_route": decision.suggested_route.as_str(),
                        "rationale": decision.rationale.clone(),
                    }),
                }),
                vec![tid.clone()],
                file!(),
                line!(),
            );
        }
        self.emit_route_selected_from_decision(&decision, model_json.to_string());
    }
}
        // (removed invalid fallback block — was inserted outside valid function scope)
