use canon_decision::RouteKind;
use canon_event::{new_error_occurred, CapabilityResult, Code, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RouteSelected, RuntimeEvent, ToolBatchSettled};
use canon_invariant::{
    decision_trace_payload, drain_persisted_store_events, invariant_violation_delta,
    invariant_violation_state, meta_invariant_verifier_sequence_contract,
    MetaInvariantVerifierSequenceStep, PersistedInvariantStoreEventKind,
};
use canon_judgment::GuardConfig;
use canon_proc_macros::must_emit;
use canon_runtime_supervisor::judgment_loop::RouteController;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use crate::{
    context::RouteContext,
    decision::{decide_from_json, RouteDecision},
    policy::{
        apply_route_policy, evaluate_route_cache, evaluate_route_dispatch, evaluate_route_emit,
        evaluate_route_emit_effects, evaluate_route_event_dispatch, evaluate_route_failure,
        evaluate_route_recovery, evaluate_route_transition, evaluate_successor_consumption,
        DeterministicRouteDecision, RouteCacheRule, RouteCacheState, RouteDispatchState,
        RouteEmitRule, RouteEmitState, RouteEventDispatchRule, RoutePolicyState,
        RoutePolicyRule,
    },
};

pub struct RouteExecutor {
    ctx: RouteContext,
    workspace: PathBuf,
    controller: RouteController,
    emitter: Option<EventEmitterHandle>,
    pending_request_id: Option<String>,
    pending_prompt: Option<String>,
    last_prompt_hash: Option<u64>,
    last_control_event_id: Option<String>,
    last_control_kind: Option<String>,
    pending_required_successor: Option<String>,
    last_route_emitted_for_control_id: Option<String>,
    last_route_prompt_hash: Option<u64>,
    last_route_selected: Option<RouteSelected>,
    force_fresh_route_once: bool,
    awaiting_control_successor: Option<String>,
    current_trigger: Option<EventId>,
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
            last_prompt_hash: None,
            last_control_event_id: None,
            last_control_kind: None,
            pending_required_successor: None,
            last_route_emitted_for_control_id: None,
            last_route_prompt_hash: None,
            last_route_selected: None,
            force_fresh_route_once: false,
            awaiting_control_successor: None,
            current_trigger: None,
        }
    }

    fn try_dispatch_route(&mut self) {
        let dispatch_eval = evaluate_route_dispatch(
            &self.ctx,
            RoutePolicyState {
                last_control_kind: self.last_control_kind.as_deref(),
                pending_required_successor: self.pending_required_successor.as_deref(),
            },
            RouteDispatchState {
                pending_request_id: self.pending_request_id.as_deref(),
                awaiting_control_successor: self.awaiting_control_successor.as_deref(),
                route_emitted_for_current_control:
                    self.last_route_emitted_for_control_id.as_deref() == self.last_control_event_id.as_deref(),
            },
        );
        if let Some(suppression) = dispatch_eval.suppression {
            if let Some(emitter) = self.emitter.as_ref() {
                let parents: Vec<_> = self.current_trigger.iter().cloned().collect();
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: "route_suppressed".to_string(),
                        payload: self.suppression_payload(
                            suppression.reason,
                            suppression.classification,
                            suppression.recovery,
                            suppression.extra,
                        ),
                    }),
                    parents.clone(),
                    file!(),
                    line!(),
                );
                if suppression.emit_stall {
                    let tid = self.current_trigger.clone().expect("route stall emit without current_trigger");
                    self.emit_route_stall(emitter, tid, suppression.reason);
                }
            }
            return;
        }
        if let Some(deterministic) = dispatch_eval.deterministic {
            let json = serde_json::json!({
                "route": deterministic.route.as_str(),
                "rationale": deterministic.rationale,
                "confidence": deterministic.confidence,
            })
            .to_string();
            self.emit_deterministic_decision(&deterministic, &json);
            return;
        }
        let llm_semantic_context = self.ctx.llm_semantic_context();
        let prompt = self.controller.build_prompt(
            &self.ctx.mission_summary,
            &self.ctx.snapshot_text(),
            &llm_semantic_context.render_router_block(),
            &self.ctx.recent_tool_results,
            &self.ctx.journal,
        );
        let prompt_hash = hash_str(&prompt);
        let mut should_force_fresh_now = false;
        let emit_eval = evaluate_route_emit(RouteEmitState {
            awaiting_control_successor: self.awaiting_control_successor.as_deref(),
            last_control_kind: self.last_control_kind.as_deref(),
            pending_required_successor: self.pending_required_successor.as_deref(),
        });
        let cache_eval = evaluate_route_cache(RouteCacheState {
            force_fresh_route_once: self.force_fresh_route_once,
            last_prompt_hash: self.last_prompt_hash,
            prompt_hash,
            pending_required_successor: self.pending_required_successor.as_deref(),
            last_route_prompt_hash: self.last_route_prompt_hash,
            route_emitted_for_current_control:
                self.last_route_emitted_for_control_id.as_deref() == self.last_control_event_id.as_deref(),
            has_cached_route: self.last_route_selected.is_some(),
            cached_route_is_observe: self
                .last_route_selected
                .as_ref()
                .is_some_and(|route| route.approved_route == "observe"),
            can_emit_route_selected: emit_eval.allowed,
        });
        if !self.force_fresh_route_once && self.last_prompt_hash == Some(prompt_hash) {
            if matches!(cache_eval.rule, RouteCacheRule::ReplayCachedRoute) {
                if self.replay_cached_route() {
                    return;
                }
            }
            if let Some(emitter) = self.emitter.as_ref() {
                match cache_eval.rule {
                    RouteCacheRule::ReplayCachedRoute => {}
                    RouteCacheRule::InvalidateCachedObserveRoute => {
                        emitter.emit_child(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "route_executor".to_string(),
                                kind: "route_suppressed".to_string(),
                                payload: self.suppression_payload(
                                    "cached observe route cannot satisfy pending route_selected obligation safely",
                                    "recoverable",
                                    "trigger_fresh_route_request",
                                    serde_json::json!({
                                        "attempted_kind": "route_selected",
                                        "cached_route": "observe",
                                    }),
                                ),
                            }),
                            self.current_trigger.iter().cloned().collect(),
                            file!(),
                            line!(),
                        );
                        emitter.emit_child(
                            RuntimeEvent::Debug(canon_event::DebugEvent {
                                source: "route_executor".to_string(),
                                kind: "recovery_event".to_string(),
                                payload: decision_trace_payload(
                                    "invalidate cached route and request fresh routing",
                                    serde_json::json!({
                                        "expected_successor": "route_selected",
                                        "recovery": "fresh_llm_route",
                                        "last_control_event_id": self.last_control_event_id,
                                        "last_control_kind": self.last_control_kind,
                                        "prompt_hash": format!("{prompt_hash:016x}"),
                                    }),
                                ),
                            }),
                            self.current_trigger.iter().cloned().collect(),
                            file!(),
                            line!(),
                        );
                        self.last_route_selected = None;
                        self.last_route_prompt_hash = None;
                        self.force_fresh_route_once = true;
                        should_force_fresh_now = true;
                    }
                    RouteCacheRule::SuppressDuplicatePrompt => {}
                    RouteCacheRule::Proceed => {}
                }
                if !should_force_fresh_now && self.pending_required_successor.as_deref() == Some("route_selected") {
                    let message = format!(
                        "invariant violation: missing required successor after {} id={}; expected=route_selected; got=route_suppressed; note=duplicate route prompt for unchanged state",
                        self.last_control_kind.as_deref().unwrap_or("unknown_control"),
                        self.last_control_event_id.as_deref().unwrap_or("unknown_event"),
                    );
                    emitter.emit_child(
                        RuntimeEvent::Code(Code {
                            delta: invariant_violation_delta(message),
                            state: invariant_violation_state(),
                        }),
                        self.current_trigger.iter().cloned().collect(),
                        file!(),
                        line!(),
                    );
                }
                if !should_force_fresh_now && matches!(cache_eval.rule, RouteCacheRule::SuppressDuplicatePrompt) {
                    let tid = self.current_trigger.clone().expect("route stall emit without current_trigger");
                    emitter.emit_child(
                        RuntimeEvent::Debug(canon_event::DebugEvent {
                            source: "route_executor".to_string(),
                            kind: "route_suppressed".to_string(),
                            payload: self.suppression_payload(
                                "duplicate route prompt for unchanged state",
                                if self.pending_required_successor.as_deref() == Some("route_selected") {
                                    "fatal"
                                } else {
                                    "recoverable"
                                },
                                if self.pending_required_successor.as_deref() == Some("route_selected") {
                                    "emit_route_selected_override|reset_event|override_event"
                                } else {
                                    "await_pending_successor_or_state_change"
                                },
                                serde_json::json!({
                                    "prompt_hash": format!("{prompt_hash:016x}"),
                                }),
                            ),
                        }),
                        vec![tid.clone()],
                        file!(),
                        line!(),
                    );
                    if self.pending_required_successor.as_deref() != Some("route_selected") {
                        self.emit_route_stall(emitter, tid, "duplicate route prompt for unchanged state");
                    }
                }
            }
            if !should_force_fresh_now && !matches!(cache_eval.rule, RouteCacheRule::Proceed) {
                return;
            }
        }
        self.last_prompt_hash = Some(prompt_hash);
        self.force_fresh_route_once = false;
        self.pending_request_id = None;
        self.pending_prompt = None;

        let fallback = self.router_disabled_fallback(prompt_hash);
        let json = serde_json::json!({
            "route": fallback.route.as_str(),
            "rationale": fallback.rationale,
            "confidence": fallback.confidence,
        })
        .to_string();
        self.emit_deterministic_decision(&fallback, &json);
    }

    fn router_disabled_fallback_rule(&self) -> crate::policy::DeterministicRouteRule {
        if self.ctx.last_invalid_plan_reason.is_some() {
            crate::policy::DeterministicRouteRule::InvalidPlanReplan
        } else if self.ctx.semantic_summary.validation_blocked_by_preconditions {
            crate::policy::DeterministicRouteRule::BlockedValidationPlan
        } else if self
            .ctx
            .semantic_summary
            .planning_preconditions
            .iter()
            .any(|line| line.contains("must_bootstrap_workspace=true"))
        {
            crate::policy::DeterministicRouteRule::MissingTargetPlan
        } else {
            crate::policy::DeterministicRouteRule::NoActionableFailureObserve
        }
    }

    fn router_disabled_fallback(&self, prompt_hash: u64) -> DeterministicRouteDecision {
        DeterministicRouteDecision {
            route: RouteKind::Plan,
            rationale: format!(
                "router_llm_disabled; route deterministically to plan for action synthesis (prompt_hash={prompt_hash:016x})"
            ),
            confidence: 0.90,
            prompt_tag: "deterministic:router_llm_disabled_plan",
            noop_reason: "route_executor_router_llm_disabled_plan",
            rule: self.router_disabled_fallback_rule(),
        }
    }

    fn replay_cached_route(&mut self) -> bool {
        let Some(route) = self.last_route_selected.clone() else {
            return false;
        };
        let emit_eval = evaluate_route_emit(RouteEmitState {
            awaiting_control_successor: self.awaiting_control_successor.as_deref(),
            last_control_kind: self.last_control_kind.as_deref(),
            pending_required_successor: self.pending_required_successor.as_deref(),
        });
        if !emit_eval.allowed {
            let reason = emit_eval
                .reason
                .unwrap_or_else(|| "illegal control emit".to_string());
            let kind = if matches!(
                emit_eval.rule,
                RouteEmitRule::DuplicateEmitBeforeSuccessor | RouteEmitRule::IllegalControlReentry
            ) {
                "duplicate_route_emit_before_successor"
            } else {
                "illegal_control_reentry"
            };
            let tid = self
                .current_trigger
                .clone()
                .expect("cached route emit without current_trigger");
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: kind.to_string(),
                        payload: self.suppression_payload(
                            &reason,
                            "recoverable",
                            "attempt_expected_successor_recovery",
                            serde_json::json!({
                                "attempted_kind": "route_selected",
                                "attempted_route": route.approved_route,
                                "replay_source": "cached_route",
                            }),
                        ),
                    }),
                    vec![tid.clone()],
                    file!(),
                    line!(),
                );
                self.emit_recovery_for_expected_successor(emitter, tid);
            }
            return true;
        }
        let tid = self
            .current_trigger
            .clone()
            .expect("cached route emit without current_trigger");
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit_with_parents(RuntimeEvent::RouteSelected(route), vec![tid], file!(), line!());
        }
        self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
        true
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
                        PersistedInvariantStoreEventKind::Loaded => {
                            "persisted_invariants_loaded".to_string()
                        }
                        PersistedInvariantStoreEventKind::Updated => {
                            "persisted_invariants_updated".to_string()
                        }
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
        let recovery_eval = evaluate_route_recovery(self.pending_required_successor.as_deref());
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
                        "last_control_event_id": self.last_control_event_id,
                        "last_control_kind": self.last_control_kind,
                        "source": "route_executor",
                    }),
                ),
            }),
            vec![trigger_id],
            file!(),
            line!(),
        );
    }

    fn emit_route_stall(&self, emitter: &EventEmitterHandle, trigger_id: EventId, reason: &str) {
        if self.pending_request_id.is_some() || self.awaiting_control_successor.is_some() {
            return;
        }
        emitter.emit_child(
            RuntimeEvent::ErrorOccurred(new_error_occurred(
                "route_stall",
                "route_executor",
                format!("route suppressed with no active successor generator: {reason}"),
                "warning",
                serde_json::json!({
                    "reason": reason,
                    "pending_required_successor": self.pending_required_successor,
                    "last_control_event_id": self.last_control_event_id,
                    "last_control_kind": self.last_control_kind,
                    "snapshot": self.ctx.snapshot_text(),
                }),
                None,
            )),
            vec![trigger_id],
            file!(),
            line!(),
        );
    }

    fn emit_invariant_violation(
        &self,
        trigger_id: &EventId,
        feature: &str,
        reason: &str,
        context: serde_json::Value,
    ) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
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
            self.last_control_kind.as_deref(),
            self.pending_required_successor.as_deref(),
            self.ctx.verify_seen,
        ) else {
            return None;
        };
        Some((
            "meta_invariant_verifier_sequence_contract",
            reason.to_string(),
            serde_json::json!({
                "event_kind": canon_event::event_kind_str(event),
                "sequence_step": step.as_str(),
                "last_control_kind": self.last_control_kind,
                "pending_required_successor": self.pending_required_successor,
                "verify_seen": self.ctx.verify_seen,
                "awaiting_control_successor": self.awaiting_control_successor,
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::RouteExecutor;
    use crate::policy::{DeterministicRouteDecision, DeterministicRouteRule};
    use crate::decision::RouteDecision;
    use canon_decision::RouteKind;
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
        let mut executor = RouteExecutor::new(PathBuf::from("/tmp/workspace"));
        let _ = executor.on_event(&route_selected("verify"), EventId::new("route-selected"));
        let outcome = executor.on_event(&loop_rewarded(), EventId::new("loop-rewarded"));
        assert!(matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }

    #[test]
    fn accepts_loop_rewarded_for_direct_conclude_route() {
        let mut executor = RouteExecutor::new(PathBuf::from("/tmp/workspace"));
        let _ = executor.on_event(&route_selected("conclude"), EventId::new("route-selected"));
        let outcome = executor.on_event(&loop_rewarded(), EventId::new("loop-rewarded"));
        assert!(!matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }

    #[test]
    fn rejects_verifier_policy_updated_before_loop_verified() {
        let mut executor = RouteExecutor::new(PathBuf::from("/tmp/workspace"));
        let _ = executor.on_event(&route_selected("verify"), EventId::new("route-selected"));
        let outcome =
            executor.on_event(&verifier_policy_updated(), EventId::new("verifier-policy-updated"));
        assert!(matches!(
            outcome,
            EventOutcome::NoOp("verifier_sequence_invariant_violation")
        ));
    }

    fn deterministic_decision(rule: DeterministicRouteRule, route: RouteKind) -> DeterministicRouteDecision {
        DeterministicRouteDecision {
            route,
            rationale: format!("{rule:?}"),
            confidence: 0.99,
            prompt_tag: "deterministic:test",
            noop_reason: "test",
            rule,
        }
    }

    #[test]
    fn deterministic_bootstrap_refresh_observe_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(
            DeterministicRouteRule::BootstrapRefreshObserve,
            RouteKind::Observe,
        ));
        assert_eq!(decision.lane, RouteKind::Observe);
        assert_eq!(decision.suggested_route, RouteKind::Observe);
        assert!(!decision.changed);
    }

    #[test]
    fn deterministic_no_semantic_progress_plan_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(
            DeterministicRouteRule::NoSemanticProgressPlan,
            RouteKind::Plan,
        ));
        assert_eq!(decision.lane, RouteKind::Plan);
        assert_eq!(decision.suggested_route, RouteKind::Plan);
        assert!(!decision.changed);
    }

    #[test]
    fn deterministic_invalid_plan_replan_is_authoritative() {
        let decision: RouteDecision = RouteExecutor::decision_from_deterministic(&deterministic_decision(
            DeterministicRouteRule::InvalidPlanReplan,
            RouteKind::Plan,
        ));
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
        self.current_trigger = Some(trigger_id.clone());
        if let Some((feature, reason, context)) = self.should_reject_verifier_sequence(event) {
            self.emit_invariant_violation(&trigger_id, feature, &reason, context);
            return EventOutcome::NoOp("verifier_sequence_invariant_violation");
        }
        let successor_eval =
            evaluate_successor_consumption(event, self.awaiting_control_successor.as_deref());
        if successor_eval.clear_awaiting_control_successor {
            self.awaiting_control_successor = None;
        }
        self.ctx.update_from_event(event, &self.workspace);
        self.record_control_state(event, &trigger_id);

        if let Some((result_count, any_failed)) = self.ctx.batch_settled.take() {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_with_parents(canon_event::RuntimeEvent::ToolBatchSettled(ToolBatchSettled {
                    tick: self.ctx.scheduler_tick,
                    result_count,
                    any_failed,
                }), vec![trigger_id.clone()], file!(), line!());
            }
            let eval = evaluate_route_event_dispatch(
                &RuntimeEvent::ToolBatchSettled(ToolBatchSettled {
                    tick: self.ctx.scheduler_tick,
                    result_count,
                    any_failed,
                }),
                self.ctx.planned_pending,
                self.ctx.pending_tool_result_ids.is_empty(),
            );
            if eval.should_dispatch {
                self.try_dispatch_route();
                return EventOutcome::NoOp("route_executor_batch_settled");
            }
        }

        if let Some(fast_path) = evaluate_route_transition(
            &self.ctx,
            RoutePolicyState {
                last_control_kind: self.last_control_kind.as_deref(),
                pending_required_successor: self.pending_required_successor.as_deref(),
            },
            Some(event),
            None,
        )
        .deterministic
        {
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

        let event_dispatch_eval =
            evaluate_route_event_dispatch(event, self.ctx.planned_pending, self.ctx.pending_tool_result_ids.is_empty());
        if matches!(event_dispatch_eval.rule, RouteEventDispatchRule::IdleDispatch) {
            if self.pending_request_id.as_deref() == Some("deterministic")
                && matches!(event, RuntimeEvent::LoopActed(_) | RuntimeEvent::LoopVerified(_))
            {
                self.pending_request_id = None;
            }
            self.try_dispatch_route();
            return EventOutcome::NoOp("route_executor_idle_dispatch");
        }

        if matches!(event_dispatch_eval.rule, RouteEventDispatchRule::RecoverableEmptyPlan) {
            if self.pending_request_id.as_deref() == Some("deterministic") {
                self.pending_request_id = None;
            }
            self.try_dispatch_route();
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
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::VerifierPolicyUpdated(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::RouteSelected(_) => EventOutcome::NoOp("route_executor_noop"),
        }
    }
}

impl RouteExecutor {
    fn emit_route_selected_from_decision(&mut self, decision: &RouteDecision, model_json: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        let route_event = RuntimeEvent::RouteSelected(RouteSelected {
            tick: self.ctx.scheduler_tick,
            approved_route: decision.lane.as_str().to_string(),
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
        self.awaiting_control_successor = match decision.lane.as_str() {
            "observe" => Some("loop_observed".to_string()),
            "plan" => Some("planning_completed".to_string()),
            "act" => Some("loop_acted".to_string()),
            "verify" => Some("verifier_policy_updated".to_string()),
            "conclude" => Some("loop_rewarded".to_string()),
            _ => None,
        };
        eprintln!(
            "[route_executor][emit] route_selected lane={} trigger={:?} last_control={:?} pending_succ={:?} awaiting={:?}",
            decision.lane.as_str(),
            self.current_trigger,
            self.last_control_kind,
            self.pending_required_successor,
            self.awaiting_control_successor,
        );
        emitter.emit_with_parents(route_event, vec![tid], file!(), line!());
        if self.pending_required_successor.as_deref() == Some("route_selected") {
            self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
        }
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
    }

    fn emit_deterministic_decision(
        &mut self,
        deterministic: &DeterministicRouteDecision,
        model_json: &str,
    ) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        eprintln!(
            "[route_executor][det] rule={} route={} trigger={:?} last_control={:?} pending_succ={:?} awaiting={:?}",
            deterministic.prompt_tag,
            deterministic.route.as_str(),
            self.current_trigger,
            self.last_control_kind,
            self.pending_required_successor,
            self.awaiting_control_successor,
        );
        let emit_eval = evaluate_route_emit(RouteEmitState {
            awaiting_control_successor: self.awaiting_control_successor.as_deref(),
            last_control_kind: self.last_control_kind.as_deref(),
            pending_required_successor: self.pending_required_successor.as_deref(),
        });
        if !emit_eval.allowed {
            let reason = emit_eval
                .reason
                .unwrap_or_else(|| "illegal control emit".to_string());
            let kind = if matches!(
                emit_eval.rule,
                RouteEmitRule::DuplicateEmitBeforeSuccessor
                    | RouteEmitRule::IllegalControlReentry
            ) {
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
            let tid = self
                .current_trigger
                .clone()
                .expect("emit_deterministic_decision called without current_trigger set");
            emitter.emit_child(
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "route_executor".to_string(),
                    kind: kind.to_string(),
                    payload,
                }),
                vec![tid.clone()],
                file!(),
                line!(),
            );
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

    fn suppression_payload(
        &self,
        reason: &str,
        classification: &str,
        recovery: &str,
        extra: serde_json::Value,
    ) -> serde_json::Value {
        let mut context = serde_json::Map::new();
        context.insert("snapshot".to_string(), serde_json::Value::String(self.ctx.snapshot_text()));
        if let Some(id) = &self.last_control_event_id {
            context.insert("last_control_event_id".to_string(), serde_json::Value::String(id.clone()));
        }
        if let Some(kind) = &self.last_control_kind {
            context.insert("last_control_kind".to_string(), serde_json::Value::String(kind.clone()));
        }
        if let Some(expected) = &self.pending_required_successor {
            context.insert("pending_required_successor".to_string(), serde_json::Value::String(expected.clone()));
        }
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
            eprintln!(
                "[route_executor][ctrl] event={} trigger={} prev_pending={:?} -> new_pending={}",
                canon_event::event_kind_str(event),
                trigger_id,
                self.pending_required_successor,
                expected,
            );
            self.last_control_event_id = Some(trigger_id.to_string());
            self.last_control_kind = Some(canon_event::event_kind_str(event).to_string());
            self.pending_required_successor = Some(expected.to_string());
        }
    }

    fn emit_decision(&mut self, model_json: &str, prompt: String) {
        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        let emit_eval = evaluate_route_emit(RouteEmitState {
            awaiting_control_successor: self.awaiting_control_successor.as_deref(),
            last_control_kind: self.last_control_kind.as_deref(),
            pending_required_successor: self.pending_required_successor.as_deref(),
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
            emitter.emit_child(
                RuntimeEvent::Debug(canon_event::DebugEvent {
                    source: "route_executor".to_string(),
                    kind: kind.to_string(),
                    payload,
                }),
                vec![tid.clone()],
                file!(),
                line!(),
            );
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
        let rules = apply_route_policy(
            &self.ctx,
            RoutePolicyState {
                last_control_kind: self.last_control_kind.as_deref(),
                pending_required_successor: self.pending_required_successor.as_deref(),
            },
            &mut decision,
        );
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
