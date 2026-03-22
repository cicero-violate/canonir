use canon_decision::{RouteKind, RouteSelection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    pub max_cycles: u32,
    pub max_repeat_lane: u32,
    pub minimum_confidence: Option<f32>,
    pub fallback_lane: RouteKind,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self { max_cycles: 64, max_repeat_lane: 3, minimum_confidence: Some(0.20), fallback_lane: RouteKind::Scan }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeSignals {
    pub context_ready: bool,
    pub has_queued_plan: bool,
    pub workspace_dirty: bool,
    pub performed_recently: bool,
    pub last_action_failed: bool,
    pub finish_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub cycle_count: u32,
    pub previous_lane: Option<RouteKind>,
    pub repeat_count: u32,
}

impl Default for GateState {
    fn default() -> Self {
        Self { cycle_count: 0, previous_lane: None, repeat_count: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub lane: RouteKind,
    pub changed: bool,
    pub note: String,
    pub should_stop: bool,
}

#[derive(Debug, Clone)]
pub struct Gatekeeper {
    cfg: GuardConfig,
    state: GateState,
}

impl Gatekeeper {
    pub fn new(cfg: GuardConfig) -> Self {
        Self { cfg, state: GateState::default() }
    }

    pub fn state(&self) -> &GateState {
        &self.state
    }

    pub fn review(&mut self, pick: &RouteSelection, signals: &RuntimeSignals) -> GateResult {
        self.state.cycle_count = self.state.cycle_count.saturating_add(1);

        if self.state.cycle_count > self.cfg.max_cycles {
            if signals.has_queued_plan {
                // Mid-batch guard: let queued work drain before reconsidering cycle cap.
                self.state.cycle_count = 0;
            } else {
                return GateResult { lane: RouteKind::Conclude, changed: true, note: "cycle cap reached; forcing conclude".to_string(), should_stop: true };
            }
        }

        let mut lane = pick.route;
        let mut changed = false;
        let mut notes: Vec<&str> = Vec::new();

        if let Some(minimum) = self.cfg.minimum_confidence {
            if let Some(confidence) = pick.confidence {
                if confidence < minimum {
                    lane = self.cfg.fallback_lane;
                    changed = true;
                    notes.push("low confidence");
                }
            }
        }

        if self.state.repeat_count > self.cfg.max_repeat_lane && !signals.has_queued_plan {
            if signals.performed_recently {
                lane = RouteKind::Validate;
                notes.push("repeat limit reached under unverified state; forcing validate");
            } else {
                lane = self.cfg.fallback_lane;
                notes.push("repeat limit reached");
            }
            changed = true;
        }

        if signals.performed_recently && !signals.has_queued_plan && lane != RouteKind::Validate {
            lane = RouteKind::Validate;
            changed = true;
            notes.push("acted_unverified=true requires validate");
        }

        if signals.last_action_failed && !signals.has_queued_plan && !signals.performed_recently && lane != RouteKind::Shape {
            lane = RouteKind::Shape;
            changed = true;
            notes.push("batch_failed requires shape for replan");
        }

        if signals.has_queued_plan && lane != RouteKind::Execute {
            lane = RouteKind::Execute;
            changed = true;
            notes.push("queued plan requires execute");
        }

        if lane == RouteKind::Execute && !signals.has_queued_plan {
            lane = RouteKind::Shape;
            changed = true;
            notes.push("execute blocked: no queued plan; select shape to produce one");
        } else if lane == RouteKind::Execute && !(signals.context_ready || signals.has_queued_plan) {
            lane = RouteKind::Scan;
            changed = true;
            notes.push("execute requires context_ready or queued plan");
        }

        if lane == RouteKind::Validate && !(signals.performed_recently || signals.workspace_dirty) {
            lane = RouteKind::Shape;
            changed = true;
            notes.push("validate requires performed_recently or workspace_dirty");
        }

        if lane == RouteKind::Conclude && !signals.finish_ready {
            lane = RouteKind::Validate;
            changed = true;
            notes.push("conclude requires finish_ready");
        }

        // Track repeats on the final gated lane (not the raw pick) for repeat_limit logic.
        if let Some(previous) = self.state.previous_lane {
            if previous == lane {
                self.state.repeat_count = self.state.repeat_count.saturating_add(1);
            } else {
                self.state.repeat_count = 1;
            }
        } else {
            self.state.repeat_count = 1;
        }
        self.state.previous_lane = Some(lane);

        let note = if notes.is_empty() { "accepted".to_string() } else { notes.join("; ") };

        GateResult { lane, changed, note, should_stop: lane == RouteKind::Conclude }
    }
}
