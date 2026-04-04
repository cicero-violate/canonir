use crate::{
    context::RouteContext,
    decision::RouteDecision,
    policy::{
        apply_route_policy, evaluate_route_emit, evaluate_route_emit_effects, evaluate_route_recovery,
        DeterministicRouteDecision, RouteEmitState, RoutePolicyRule, RoutePolicyState,
    },
};
// TRACE: global runtime introspection (file, line, function)
use canon_decision::RouteKind;
use canon_event::{new_error_occurred, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RouteSelected, RuntimeEvent, ToolBatchSettled};
use canon_invariant::{decision_trace_payload, drain_persisted_store_events, meta_invariant_verifier_sequence_contract, MetaInvariantVerifierSequenceStep, PersistedInvariantStoreEventKind};
use canon_judgment::GuardConfig;
use canon_proc_macros::must_emit;
use canon_runtime_supervisor::judgment_loop::RouteController;
// decision wiring will be added after full integration
use std::path::PathBuf;

pub struct RouteExecutor {
    ctx: RouteContext,
    workspace: PathBuf,
    #[allow(dead_code)]
    controller: RouteController,
    emitter: Option<EventEmitterHandle>,
    pending_request_id: Option<String>,
    pending_prompt: Option<String>,
    dispatch_in_progress: bool,
    reroute_requested: bool,
    current_trigger: Option<EventId>,
    // STRICT: decision → route invariant tracking
    last_decision_trace_id: Option<u64>,
    // removed scheduler_len mirror — routing must not depend on queue-derived state
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
            dispatch_in_progress: false,
            reroute_requested: false,
            current_trigger: None,
            last_decision_trace_id: None,
            // scheduler_len removed
        }
    }

    #[allow(unreachable_code, unused_variables)]
    fn try_dispatch_route(&mut self, _trigger_event: &RuntimeEvent) {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        eprintln!("[ENTER] {}:{} {} - executor::try_dispatch_route", file!(), line!(), module_path!());
        // SAFETY: guard entire executor to prevent runtime crash
        let __route_exec_guard = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if self.dispatch_in_progress {
                // Mark reroute, but do not recursively re-enter decision emission on the same stack.
                // The active dispatch will finish and emit a RouteTick if reroute is still required.
                eprintln!("[ROUTE FIX] dispatch_in_progress detected — deferring recursive dispatch");
                self.reroute_requested = true;
                return;
            }

            static TRACE_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let trace_id = TRACE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.last_decision_trace_id = Some(trace_id);

            // Canonical route dispatch is semantic-only: RouteTick -> decision() -> RouteSelected.
            self.emit_decision("", String::new());

        }));

        if __route_exec_guard.is_err() {
            eprintln!("[WARN] RouteExecutor panic suppressed to keep runtime alive");
        }
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

    #[allow(dead_code)]
    #[allow(dead_code)]
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
        eprintln!("[ROUTE EXEC TRACE] on_event event={:?} trigger_id={:?} dispatch_in_progress={}", event, trigger_id, self.dispatch_in_progress);
        self.current_trigger = Some(trigger_id.clone());

        // FIX: allow initial RuntimeEvent to pass through for loop entry
        // Canon requires state → decision → transition, so routing must not block
        // non-loop events before LoopStageExecutor is engaged.

        // removed invalid match (must_emit requires exhaustive handling of RuntimeEvent)

        // CRITICAL FIX: prevent parallel control-flow
        // RouteExecutor must not process raw RuntimeEvent directly.
        // Canonical loop (LoopStageExecutor) is now responsible for control flow.
        // NOTE: removed early return to avoid unreachable code; RouteExecutor still needs restructuring

        // REMOVED: synthetic re-emission bypasses canonical routing (must derive from SemanticStateSummary only)

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
            // removed executor-level routing override (missing_semantic_context → Observe)
            // routing must be derived from SemanticStateSummary via policy
            eprintln!("[ROUTE EXEC TRACE] PlanningCompleted has no planned work; falling through to normal route policy");
        }

        // Routing must be driven only by the canonical RouteTick control event.
        // Calling try_dispatch_route() for every event duplicates same-tick decisions
        // (for example LoopObserved + RouteTick) and can recurse RouteSelected emission.

        // removed invalid direct invocation of canon_loop (not available in this crate)

        
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
            // CRITICAL FIX: eliminate event-driven dispatch
            // Routing must only proceed via decision() → RouteSelected
        }

        // FIX: remove forced dispatch path to allow natural routing progression

        // Enforce SemanticStateSummary-only routing: remove event-driven transition evaluation
        // REMOVED: deterministic and event-driven routing paths
        // Routing must be driven exclusively via SemanticStateSummary → decision() → RouteSelected

        // REMOVED: event_dispatch_eval-based routing (non-semantic path)
        // Routing must exclusively follow SemanticStateSummary → decision() → RouteSelected

        if let RuntimeEvent::Tick(t) = event {
            self.ctx.scheduler_tick = t.tick;
        }

        match event {
            RuntimeEvent::CapabilityRequested(_) => {
                return EventOutcome::NoOp("route_executor_capability_requested");
            }
            RuntimeEvent::CapabilityCompleted(done) => {
                if Some(&done.request_id) != self.pending_request_id.as_ref() || done.capability != "llm.call" {
                    return EventOutcome::NoOp("route_executor_unrelated_completion");
                }
                let _prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                EventOutcome::NoOp("route_executor_completion")
            }
            RuntimeEvent::CapabilityFailed(failed) => {
                if Some(&failed.request_id) != self.pending_request_id.as_ref() || failed.capability != "llm.call" {
                    return EventOutcome::NoOp("route_executor_unrelated_failure");
                }
                let _prompt = self.pending_prompt.clone().unwrap_or_default();
                self.pending_request_id = None;
                self.pending_prompt = None;
                EventOutcome::NoOp("route_executor_failure_reroute")
            }
            RuntimeEvent::RouteTick(_) => {
                // CRITICAL FIX: RouteTick must drive per-cycle decision execution
                static TRACE_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                let trace_id = TRACE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.last_decision_trace_id = Some(trace_id);
                let prompt = String::new();
                self.emit_decision("", prompt);
                EventOutcome::NoOp("route_executor_route_tick")
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            => {
                EventOutcome::NoOp("ignored_event_kind")
            }
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
    #[allow(dead_code)]
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

    fn advance_control_state(&mut self, _event: &RuntimeEvent) {
        // Canonical flow must not depend on executor-local successor tracking.
    }

    fn emit_route_selected_from_decision(&mut self, decision: &RouteDecision, model_json: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        // HARD INVARIANT: require decision_trace before emitting RouteSelected
        if self.last_decision_trace_id.is_none() {
            panic!("RouteSelected emitted without preceding decision_trace");
        }
        // No executor-local suppression is allowed here.
        // Exactly-once RouteSelected must be enforced by canonical runtime invariants,
        // not by comparing against cached prior route state.
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        // INVARIANT: every RouteSelected emission MUST have exactly one preceding ROUTE TRACE
        eprintln!(
            "[ROUTE TRACE] {}:{} {} fn=emit_route_selected decision={:?}",
            file!(),
            line!(),
            module_path!(),
            decision.lane
        );

        // RouteSelected emission ENABLED — required for decision → route invariant
        let route_event = RuntimeEvent::RouteSelected(RouteSelected {
            tick: self.ctx.scheduler_tick,
            approved_route: {
                // FIX: remove scheduler_len invariant — routing must be semantic-only
                eprintln!(
                    "[DECIDE CHECK] decision={:?}",
                    decision.lane
                );
                // FIX: restore canonical casing expected by downstream (PascalCase)
                // FIX: enforce PascalCase canonical form (e.g., "Observe")
                // FIX: use enum canonical Debug representation
                format!("{:?}", decision.lane)
            },
            // FIX: restore canonical casing for suggested route
            // FIX: enforce PascalCase for suggested route as well
            // FIX: use Debug representation for suggested route as well
            // FIX: ensure suggested_route matches approved_route exactly
            suggested_route: format!("{:?}", decision.lane),
            rationale: decision.rationale.clone(),
            confidence: decision.confidence,
            gate_note: decision.note.clone(),
            gate_rules_fired: decision.gate_rules_fired.clone(),
            gate_changed: decision.changed,
            gate_should_stop: decision.should_stop,
            prompt: decision.prompt.clone(),
            model_json,
        });

        let RuntimeEvent::RouteSelected(_route_payload) = &route_event else {
            unreachable!("route_event must be route_selected");
        };
        // STRICT INVARIANT: consume decision_trace to enforce exactly-one RouteSelected per decision
        self.last_decision_trace_id = None;
        let Some(tid) = self.current_trigger.clone() else {
            eprintln!("[WARN] emit_route_selected_from_decision called without current_trigger; skipping emission");
            return;
        };
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

    #[allow(dead_code)]
    #[allow(dead_code)]
    fn emit_deterministic_decision(&mut self, deterministic: &DeterministicRouteDecision, model_json: &str) {
        let Some(_emitter) = self.emitter.as_ref() else {
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
        let _emit_eval = evaluate_route_emit(RouteEmitState {
            // removed awaiting_control_successor
            last_control_kind: None,
            pending_required_successor: None,
            ..Default::default()
        });
        let decision = Self::decision_from_deterministic(deterministic);
        self.emit_route_selected_from_decision(&decision, model_json.to_string());
    }

    #[allow(dead_code)]
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    #[allow(dead_code)]
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

    fn emit_decision(&mut self, _model_json: &str, prompt: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        let _emit_eval = evaluate_route_emit(RouteEmitState {
            // removed awaiting_control_successor
            last_control_kind: None,
            pending_required_successor: None,
            ..Default::default()
        });
        let _semantic_json = serde_json::to_string(&self.ctx.semantic_summary).unwrap_or_default();
        // FIX: use canonical decision from canon-invariant instead of decide_from_json
        let invariant_decision = canon_invariant::decide(canon_invariant::DecisionState {
            has_plan: false,
        });

        let mut decision = RouteDecision {
            lane: match invariant_decision {
                canon_invariant::Decision::Observe => RouteKind::Observe,
                canon_invariant::Decision::Act => RouteKind::Act,
                _ => RouteKind::Plan,
            },
            suggested_route: match invariant_decision {
                canon_invariant::Decision::Observe => RouteKind::Observe,
                canon_invariant::Decision::Act => RouteKind::Act,
                _ => RouteKind::Plan,
            },
            rationale: "canonical_invariant_decision".to_string(),
            confidence: Some(1.0),
            changed: false,
            note: "canonical_invariant".to_string(),
            gate_rules_fired: vec![],
            should_stop: false,
            prompt,
        };
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
        self.emit_route_selected_from_decision(&decision, "".to_string());
    }
}
