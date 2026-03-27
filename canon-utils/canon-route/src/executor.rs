use canon_decision::RouteKind;
use canon_event::{new_error_occurred, CapabilityResult, Code, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, LlmCall, RouteSelected, RuntimeEvent, ToolBatchSettled};
use canon_invariant::{decision_trace_payload, invariant_violation_delta, invariant_violation_state};
use canon_judgment::GuardConfig;
use canon_proc_macros::must_emit;
use canon_runtime_supervisor::judgment_loop::RouteController;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use uuid::Uuid;

use crate::{
    context::RouteContext,
    decision::{decide_from_json, RouteDecision},
    policy::{
        apply_route_policy, evaluate_route_cache, evaluate_route_dispatch, evaluate_route_emit,
        evaluate_route_emit_effects, evaluate_route_event_dispatch, evaluate_route_failure,
        evaluate_route_recovery, evaluate_route_transition, evaluate_successor_consumption,
        RouteCacheRule, RouteCacheState, RouteDispatchState, RouteEmitRule, RouteEmitState,
        RouteEventDispatchRule, RoutePolicyState,
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
            self.emit_decision(&json, deterministic.prompt_tag.to_string());
            return;
        }
        let prompt = self.controller.build_prompt(&self.ctx.mission_summary, &self.ctx.snapshot_text(), &self.ctx.recent_tool_results, &self.ctx.journal);
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
            if let Some(emitter) = self.emitter.as_ref() {
                match cache_eval.rule {
                    RouteCacheRule::ReplayCachedRoute => {
                        if let Some(route) = self.last_route_selected.clone() {
                            let tid = self.current_trigger.clone().expect("cached route emit without current_trigger");
                            emitter.emit_with_parents(RuntimeEvent::RouteSelected(route), vec![tid], file!(), line!());
                            self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
                            return;
                        }
                    }
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
        let request_id = format!("route-{}", Uuid::new_v4());
        self.pending_request_id = Some(request_id.clone());
        self.pending_prompt = Some(prompt.clone());
        self.last_prompt_hash = Some(prompt_hash);
        self.force_fresh_route_once = false;
        if let Some(emitter) = self.emitter.as_ref() {
            let tid = self.current_trigger.clone().expect("try_dispatch_route called without current_trigger");
            emitter.emit_with_parents(canon_event::RuntimeEvent::Llm(LlmCall {
                request_id,
                prompt,
                role: Some("router".to_string()),
                agent_id: Some("router_chatgpt_group".to_string()),
                dispatched: true,
                system: None,
                system_prompt_id: None,
                context_base: None,
                context_base_id: None,
                prompt_base_id: None,
                prev_prompt_id: None,
            }), vec![tid], file!(), line!());
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
        let successor_eval =
            evaluate_successor_consumption(event, self.awaiting_control_successor.as_deref());
        if successor_eval.clear_awaiting_control_successor {
            self.awaiting_control_successor = None;
        }
        // Always accumulate state.
        self.ctx.update_from_event(event, &self.workspace);
        self.record_control_state(event, &trigger_id);

        // Check if the batch just settled — emit the event and trigger routing.
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
            self.emit_decision(&json, fast_path.prompt_tag.to_string());
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
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::RouteSelected(_) => EventOutcome::NoOp("route_executor_noop"),
        }
    }
}

impl RouteExecutor {
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
                "verify" => Some("loop_verified"),
                "conclude" => Some("loop_rewarded"),
                _ => None,
            },
            RuntimeEvent::LoopObserved(_) => Some("route_selected"),
            RuntimeEvent::PlanningCompleted(_) => Some("route_selected"),
            RuntimeEvent::LoopActed(_) => Some("route_selected"),
            RuntimeEvent::LoopVerified(_) => Some("loop_rewarded"),
            RuntimeEvent::LoopRewarded(_) => Some("route_selected"),
            _ => None,
        };
        if let Some(expected) = next {
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
        let _rules = apply_route_policy(
            &self.ctx,
            RoutePolicyState {
                last_control_kind: self.last_control_kind.as_deref(),
                pending_required_successor: self.pending_required_successor.as_deref(),
            },
            &mut decision,
        );
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
            model_json: model_json.to_string(),
        });
        let RuntimeEvent::RouteSelected(route_payload) = &route_event else {
            unreachable!("route_event must be route_selected");
        };
        self.last_route_prompt_hash = Some(hash_str(&decision.prompt));
        self.last_route_selected = Some(route_payload.clone());
        let tid = self.current_trigger.clone().expect("emit_decision called without current_trigger set");
        self.awaiting_control_successor = match decision.lane.as_str() {
            "observe" => Some("loop_observed".to_string()),
            "plan" => Some("planning_completed".to_string()),
            "act" => Some("loop_acted".to_string()),
            "verify" => Some("loop_verified".to_string()),
            "conclude" => Some("loop_rewarded".to_string()),
            _ => None,
        };
        emitter.emit_with_parents(route_event, vec![tid], file!(), line!());
        if self.pending_required_successor.as_deref() == Some("route_selected") {
            self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
        }
        let emit_effects = evaluate_route_emit_effects(&decision);
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
}
