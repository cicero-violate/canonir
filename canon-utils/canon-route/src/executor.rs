use canon_decision::RouteKind;
use canon_event::{CapabilityResult, Code, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, LlmCall, RouteSelected, RuntimeEvent, ToolBatchSettled};
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
    helpers::heuristic_route_json,
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
            current_trigger: None,
        }
    }

    fn try_dispatch_route(&mut self) {
        if self.ctx.halted {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: "route_suppressed".to_string(),
                        payload: self.suppression_payload("runtime halted", "fatal", "reset_event|override_event|recovery_event", serde_json::json!({})),
                    }),
                    self.current_trigger.iter().cloned().collect(),
                    file!(),
                    line!(),
                );
            }
            return;
        }
        if !self.ctx.context_ready {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: "route_suppressed".to_string(),
                        payload: self.suppression_payload("context not ready", "recoverable", "await_context", serde_json::json!({})),
                    }),
                    self.current_trigger.iter().cloned().collect(),
                    file!(),
                    line!(),
                );
            }
            return;
        }
        if self.pending_request_id.is_some() {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: "route_suppressed".to_string(),
                        payload: self.suppression_payload(
                            "pending request already in flight",
                            "recoverable",
                            "await_capability_completed",
                            serde_json::json!({ "pending_request_id": self.pending_request_id }),
                        ),
                    }),
                    self.current_trigger.iter().cloned().collect(),
                    file!(),
                    line!(),
                );
            }
            return;
        }
        if self.pending_required_successor.as_deref() == Some("route_selected")
            && self.last_route_emitted_for_control_id.as_deref() == self.last_control_event_id.as_deref()
        {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "route_executor".to_string(),
                        kind: "route_suppressed".to_string(),
                        payload: self.suppression_payload(
                            "route already emitted for current control event",
                            "recoverable",
                            "await_successor",
                            serde_json::json!({}),
                        ),
                    }),
                    self.current_trigger.iter().cloned().collect(),
                    file!(),
                    line!(),
                );
            }
            return;
        }
        // Deterministic fast-path disabled: always fall through to router LLM.
        let prompt = self.controller.build_prompt(&self.ctx.mission_summary, &self.ctx.snapshot_text(), &self.ctx.recent_tool_results, &self.ctx.journal);
        let prompt_hash = hash_str(&prompt);
        let mut should_force_fresh_now = false;
        if !self.force_fresh_route_once && self.last_prompt_hash == Some(prompt_hash) {
            if let Some(emitter) = self.emitter.as_ref() {
                if self.pending_required_successor.as_deref() == Some("route_selected")
                    && self.last_route_prompt_hash == Some(prompt_hash)
                    && self.last_route_emitted_for_control_id.as_deref() != self.last_control_event_id.as_deref()
                    && self.can_emit_route_selected().is_ok()
                {
                    if let Some(route) = self.last_route_selected.clone() {
                        if route.approved_route == "observe" {
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
                                            "cached_route": route.approved_route,
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
                        } else {
                            let tid = self.current_trigger.clone().expect("cached route emit without current_trigger");
                            emitter.emit_with_parents(RuntimeEvent::RouteSelected(route), vec![tid], file!(), line!());
                            self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
                            return;
                        }
                    }
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
                if !should_force_fresh_now {
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
                        self.current_trigger.iter().cloned().collect(),
                        file!(),
                        line!(),
                    );
                }
            }
            if !should_force_fresh_now {
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

    fn can_emit_route_selected(&self) -> Result<(), String> {
        if self.last_control_kind.as_deref() == Some("route_selected") {
            return Err(format!(
                "illegal_control_reentry; attempted=route_selected; last_control_kind=route_selected; expected_successor={}",
                self.pending_required_successor.as_deref().unwrap_or("unknown")
            ));
        }
        if let Some(expected) = self.pending_required_successor.as_deref() {
            if expected != "route_selected" {
                return Err(format!(
                    "illegal_control_emit; attempted=route_selected; last_control_kind={}; expected_successor={}",
                    self.last_control_kind.as_deref().unwrap_or("unknown"),
                    expected
                ));
            }
        }
        Ok(())
    }

    fn emit_recovery_for_expected_successor(&self, emitter: &EventEmitterHandle, trigger_id: EventId) {
        let Some(expected) = self.pending_required_successor.as_deref() else {
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
            self.try_dispatch_route();
            return EventOutcome::NoOp("route_executor_batch_settled");
        }

        // Event-driven dispatch: fire immediately when the system becomes idle or context arrives.
        // This eliminates up to 1s of RouteTick latency on each transition.
        // After a `done` action, force verify so finish_ready can be set correctly.
        if let RuntimeEvent::LoopActed(a) = event {
            if a.action_kind == "done" && self.ctx.planned_pending == 0 {
                if self.pending_request_id.as_deref() == Some("deterministic") {
                    self.pending_request_id = None;
                }
                let json = serde_json::json!({
                    "route": "verify",
                    "rationale": "done action executed; verify to confirm goal completion",
                    "confidence": 0.99,
                })
                .to_string();
                self.emit_decision(&json, "deterministic:done_verify".to_string());
                return EventOutcome::NoOp("route_executor_done_verify");
            }
        }

        let should_try = matches!(event, RuntimeEvent::LoopObserved(_) | RuntimeEvent::LoopActed(_) | RuntimeEvent::LoopVerified(_));
        if should_try {
            let idle = self.ctx.planned_pending == 0 && self.ctx.pending_tool_result_ids.is_empty();
            if idle {
                // Only clear the deterministic sentinel on real state transitions (acted/verified),
                // not on bare observation. Clearing on every LoopObserved caused idle_plan to
                // re-fire on every tick, producing thousands of RouteSelected(plan) events while
                // the plan LLM response guard (last_handled_observed_hash) returned Noop — a
                // closed loop with zero state transition.
                if self.pending_request_id.as_deref() == Some("deterministic")
                    && matches!(event, RuntimeEvent::LoopActed(_) | RuntimeEvent::LoopVerified(_))
                {
                    self.pending_request_id = None;
                }
                self.try_dispatch_route();
                return EventOutcome::NoOp("route_executor_idle_dispatch");
            }
        }

        // Planning completed — planned_pending > 0 and all tools resolved.
        // batch_settled is suppressed for plan-only batches so we trigger here instead.
        if let RuntimeEvent::PlanningCompleted(_) = event {
            if self.ctx.planned_pending > 0 && self.ctx.pending_tool_result_ids.is_empty() {
                if self.pending_request_id.as_deref() == Some("deterministic") {
                    self.pending_request_id = None;
                }
                self.try_dispatch_route();
                return EventOutcome::NoOp("route_executor_plan_dispatch");
            }
        }

        // Track tick counter from Tick events (replaces RouteTick).
        if let RuntimeEvent::Tick(t) = event {
            self.ctx.scheduler_tick = t.tick;
        }

        // Handle routing LLM completion/failure.
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
                let model_json = heuristic_route_json(&self.ctx);
                self.emit_decision(&model_json, prompt);
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
        if let Err(reason) = self.can_emit_route_selected() {
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
                    kind: "illegal_control_reentry".to_string(),
                    payload,
                }),
                vec![tid.clone()],
                file!(),
                line!(),
            );
            self.emit_recovery_for_expected_successor(emitter, tid);
            return;
        }
        let decision = decide_from_json(&self.ctx, model_json, prompt.clone(), &mut self.controller).unwrap_or_else(|e| RouteDecision {
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
        emitter.emit_with_parents(route_event, vec![tid], file!(), line!());
        if self.pending_required_successor.as_deref() == Some("route_selected") {
            self.last_route_emitted_for_control_id = self.last_control_event_id.clone();
        }
        // "observe" produces LoopObserved as its follow-up event. LoopObserved is intentionally
        // excluded from the sentinel-clear in on_event (to prevent plan-spam loops), so the
        // deterministic sentinel would never be cleared for an observe route — causing a deadlock
        // where try_dispatch_route returns early on every LoopObserved. Clear it here instead.
        if decision.lane.as_str() == "observe" {
            self.pending_request_id = None;
            self.pending_prompt = None;
        }
        // Halt immediately when routing to conclude so that backlogged LoopObserved events
        // in the bus queue don't each trigger another RouteSelected(conclude) before the
        // LoopRewarded event propagates back to set ctx.halted.
        if decision.lane.as_str() == "conclude" {
            self.ctx.halted = true;
        }
    }
}
