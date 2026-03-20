use canon_event::{new_error_occurred, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter, LoopRewarded, LoopVerified};

pub struct RewardConsumer {
    emitter: Option<EventEmitterHandle>,
    errors_before: usize,
    stagnant_ticks: u32,
    last_action_kind: String,
    halted: bool,
    last_trace_id: Option<String>,
    last_execution_id: Option<String>,
    last_verify_span_id: Option<String>,
}

impl RewardConsumer {
    pub fn new() -> Self {
        Self {
            emitter: None,
            errors_before: 0,
            stagnant_ticks: 0,
            last_action_kind: "no_op".to_string(),
            halted: false,
            last_trace_id: None,
            last_execution_id: None,
            last_verify_span_id: None,
        }
    }
}

impl EventConsumer for RewardConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::LoopObserved(observed) => {
                self.errors_before = observed.error_count;
            }
            CanonEvent::LoopActed(acted) => {
                self.last_action_kind = acted.action_kind.clone();
            }
            CanonEvent::LoopVerified(verified) => {
                self.last_trace_id = verified.trace_id.clone();
                self.last_execution_id = verified.execution_id.clone();
                self.last_verify_span_id = verified.span_id.clone();
                self.handle_verified(verified);
            }
            CanonEvent::Debug(debug) if debug.kind == "route_selected" => {
                let lane = debug
                    .payload
                    .get("approved_route")
                    .or_else(|| debug.payload.get("lane"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if lane == "conclude" {
                    self.emit_forced_halt(debug.payload.get("tick").and_then(|v| v.as_u64()));
                }
            }
            _ => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}

impl RewardConsumer {
    fn emit_forced_halt(&mut self, tick: Option<u64>) {
        if self.halted {
            return;
        }
        self.halted = true;
        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopRewarded(LoopRewarded {
                tick: tick.unwrap_or(0),
                reward: 1.0,
                errors_before: self.errors_before,
                errors_after: self.errors_before,
                stagnant_ticks: self.stagnant_ticks,
                halt: true,
                trace_id: self.last_trace_id.clone(),
                execution_id: self.last_execution_id.clone(),
                span_id: Some(uuid::Uuid::new_v4().to_string()),
                parent_span_id: self.last_verify_span_id.clone(),
            }));
        }
    }

    fn handle_verified(&mut self, verified: &LoopVerified) {
        // Already halted — stop processing to prevent repeated halt events on tlog replay
        if self.halted {
            return;
        }

        let errors_after = verified.error_count;
        let mut reward = (self.errors_before as i32 - errors_after as i32) as f32;
        if verified.passed && self.last_action_kind != "no_op" {
            reward += 0.5;
        }
        if !verified.passed {
            reward -= 1.0;
        }

        if self.last_action_kind == "done" {
            if let Some(emitter) = self.emitter.as_ref() {
                emitter.emit(CanonEvent::LoopRewarded(LoopRewarded {
                    tick: verified.tick,
                    reward: 1.0,
                    errors_before: self.errors_before,
                    errors_after,
                    stagnant_ticks: 0,
                    halt: true,
                    trace_id: self.last_trace_id.clone(),
                    execution_id: self.last_execution_id.clone(),
                    span_id: Some(uuid::Uuid::new_v4().to_string()),
                    parent_span_id: self.last_verify_span_id.clone(),
                }));
            }
            return;
        }

        // no_op means "nothing to do" — don't penalise as stagnant.
        if self.last_action_kind == "no_op" {
            self.stagnant_ticks = 0;
        } else if reward <= 0.0 {
            self.stagnant_ticks = self.stagnant_ticks.saturating_add(1);
        } else {
            self.stagnant_ticks = 0;
        }

        let halt = self.stagnant_ticks > 5;
        if halt {
            self.halted = true;
        }
        let payload = LoopRewarded {
            tick: verified.tick,
            reward,
            errors_before: self.errors_before,
            errors_after,
            stagnant_ticks: self.stagnant_ticks,
            halt,
            trace_id: self.last_trace_id.clone(),
            execution_id: self.last_execution_id.clone(),
            span_id: Some(uuid::Uuid::new_v4().to_string()),
            parent_span_id: self.last_verify_span_id.clone(),
        };

        if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(CanonEvent::LoopRewarded(payload));
            if halt {
                emitter.emit(CanonEvent::ErrorOccurred(new_error_occurred(
                    "reward_halt",
                    "reward",
                    "stagnant:halt",
                    "error",
                    serde_json::json!({
                        "tick": verified.tick,
                        "stagnant_ticks": self.stagnant_ticks,
                        "errors_before": self.errors_before,
                        "errors_after": errors_after,
                    }),
                    self.last_trace_id.clone(),
                )));
            }
        }
    }
}
