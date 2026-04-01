use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use std::time::{Duration, Instant};

pub struct DiagnosticsConsumer {
    emitter: Option<EventEmitterHandle>,
    last_trigger: Option<Instant>,
    last_parent: Option<String>,
    cooldown_ticks: u64,
    min_stagnant_ticks: u32,
    min_failure_burst: u64,
    failure_burst: u64,
}

impl DiagnosticsConsumer {
    pub fn new() -> Self {
        let cooldown_ticks = std::env::var("ANALYST_COOLDOWN_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(50);
        let min_stagnant_ticks = std::env::var("ANALYST_STAGNANT_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
        let min_failure_burst = std::env::var("ANALYST_MIN_FAILURE_BURST").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
        Self { emitter: None, last_trigger: None, last_parent: None, cooldown_ticks, min_stagnant_ticks, min_failure_burst, failure_burst: 0 }
    }

    fn cooldown_active(&self) -> bool {
        if let Some(last) = self.last_trigger {
            // Convert cooldown ticks ~ seconds; conservative 1s per tick fallback.
            let wait = Duration::from_secs(self.cooldown_ticks);
            return last.elapsed() < wait;
        }
        false
    }
}

impl EventConsumer for DiagnosticsConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "diagnostics_consumer"
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        // Signal extraction
        let mut w = false;
        let mut v = false;
        let mut z = false;
        let mut p = false;
        let mut reason: Option<&'static str> = None;

        match event {
            // 🔥 GLOBAL FALLBACK: ensure PlanningCompleted always leads to execution
            RuntimeEvent::PlanningCompleted(p) => {
                return EventOutcome::emit(
                    RuntimeEvent::RequestDispatch(canon_event::RequestDispatch {
                        agent_id: "planner".to_string(),
                        dispatch_id: format!("dispatch-{}", p.tick),
                        parent_request_id: "".to_string(),
                        task_prompt: "".to_string(),
                        task_kind: "Act".to_string(),
                        deps: vec![],
                        workspace_scope: None,
                        dispatched: false,
                    }),
                    file!(),
                    line!(),
                );
            }
            // 🔥 CRITICAL: RouteSelected must trigger execution
            RuntimeEvent::RouteSelected(route) => {
                let dispatch = canon_event::RequestDispatch {
                    agent_id: "planner".to_string(),
                    dispatch_id: format!("dispatch-{}", route.tick),
                    parent_request_id: "".to_string(),
                    task_prompt: route.prompt.clone(),
                    task_kind: route.approved_route.clone(),
                    deps: vec![],
                    workspace_scope: None,
                    dispatched: false,
                };

                return EventOutcome::emit(
                    RuntimeEvent::RequestDispatch(dispatch),
                    file!(),
                    line!(),
                );
            }
            RuntimeEvent::ErrorOccurred(err) => {
                if err.source == "watchdog" {
                    w = true;
                    reason = Some("watchdog_stall");
                }
                if err.source == "invariant-engine" || err.kind.contains("invariant") {
                    v = true;
                    reason = Some("invariant_violation");
                }
                if err.kind == "capability_failed" || err.severity == "error" {
                    self.failure_burst = self.failure_burst.saturating_add(1);
                } else {
                    self.failure_burst = 0;
                }
            }
            RuntimeEvent::InvariantDiscovered(_) => {
                v = true;
                reason = Some("invariant_discovered");
            }
            RuntimeEvent::LoopRewarded(r) => {
                if r.stagnant_ticks >= self.min_stagnant_ticks {
                    z = true;
                    reason = Some("stagnant_ticks");
                }
                if r.halt {
                    z = true;
                    reason = Some("halted");
                }
            }
            RuntimeEvent::VerifierPolicyUpdated(vr) => {
                if vr.actionable_failure {
                    p = true;
                    reason = Some("verifier_policy_updated_failure");
                }
            }
            RuntimeEvent::CapabilityFailed(_) => {
                self.failure_burst = self.failure_burst.saturating_add(1);
            }
            RuntimeEvent::CapabilityCompleted(_) => {
                self.failure_burst = 0;
            }
            _ => {
                eprintln!("[DIAGNOSTICS DEBUG] event debug: {:?}", event);
            }
        }

        let u = self.failure_burst >= self.min_failure_burst;
        let should_run = (w || v || z || u || p) && !self.cooldown_active();
        let parent_id_str = trigger_id.as_str().to_string();
        let dedupe = self.last_parent.as_ref().map(|p| p == &parent_id_str).unwrap_or(false);
        let fatal_invariant = matches!(event, RuntimeEvent::ErrorOccurred(err) if err.kind == "invariant_violation");

        if should_run && !dedupe {
            if let Some(em) = self.emitter.as_ref() {
                let why = reason.unwrap_or("diagnostic_trigger");
                if !fatal_invariant {
                    let dispatch = canon_event::RequestDispatch {
                        dispatch_id: format!("diagnostics-{}", canon_event::new_event_id()),
                        parent_request_id: parent_id_str.clone(),
                        agent_id: "canon-analyst".to_string(),
                        task_prompt: format!("Run canon-analyst: reason={why}; inspect recent events and failures."),
                        task_kind: "diagnostics".to_string(),
                        deps: vec![],
                        workspace_scope: None,
                        dispatched: true,
                    };
                    em.emit_child(RuntimeEvent::RequestDispatch(dispatch), vec![trigger_id.clone()], file!(), line!());
                }
                em.emit_child(
                    RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                        "diagnostics_triggered",
                        "diagnostics_consumer",
                        format!("diagnostics triggered: {}", why),
                        "warning",
                        serde_json::json!({
                            "w": w, "v": v, "z": z, "u": u, "p": p,
                            "fatal_invariant": fatal_invariant,
                            "failure_burst": self.failure_burst,
                            "stagnant_threshold": self.min_stagnant_ticks,
                        }),
                        Some(parent_id_str.clone()),
                    )),
                    vec![trigger_id.clone()],
                    file!(),
                    line!(),
                );
                self.last_trigger = Some(Instant::now());
                self.last_parent = Some(parent_id_str);
                self.failure_burst = 0;
                return EventOutcome::NoOp("diagnostics_triggered");
            }
        }

        EventOutcome::NoOp("diagnostics_noop")
    }
}
