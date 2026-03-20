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
        Self {
            max_cycles: 12,
            max_repeat_lane: 3,
            minimum_confidence: Some(0.20),
            fallback_lane: RouteKind::Scan,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeSignals {
    pub context_ready: bool,
    pub performed_recently: bool,
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
        Self {
            cycle_count: 0,
            previous_lane: None,
            repeat_count: 0,
        }
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
        Self {
            cfg,
            state: GateState::default(),
        }
    }

    pub fn state(&self) -> &GateState {
        &self.state
    }

    pub fn review(&mut self, pick: &RouteSelection, signals: &RuntimeSignals) -> GateResult {
        self.state.cycle_count = self.state.cycle_count.saturating_add(1);

        if self.state.cycle_count > self.cfg.max_cycles {
            return GateResult {
                lane: RouteKind::Conclude,
                changed: true,
                note: "cycle cap reached; forcing conclude".to_string(),
                should_stop: true,
            };
        }

        if let Some(previous) = self.state.previous_lane {
            if previous == pick.route {
                self.state.repeat_count = self.state.repeat_count.saturating_add(1);
            } else {
                self.state.repeat_count = 1;
            }
        } else {
            self.state.repeat_count = 1;
        }

        self.state.previous_lane = Some(pick.route);

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

        if self.state.repeat_count > self.cfg.max_repeat_lane {
            lane = self.cfg.fallback_lane;
            changed = true;
            notes.push("repeat limit reached");
        }

        if lane == RouteKind::Execute && !signals.context_ready {
            lane = RouteKind::Scan;
            changed = true;
            notes.push("execute requires context_ready");
        }

        if lane == RouteKind::Validate && !signals.performed_recently {
            lane = RouteKind::Shape;
            changed = true;
            notes.push("validate requires performed_recently");
        }

        if lane == RouteKind::Conclude && !signals.finish_ready {
            lane = RouteKind::Validate;
            changed = true;
            notes.push("conclude requires finish_ready");
        }

        let note = if notes.is_empty() {
            "accepted".to_string()
        } else {
            notes.join("; ")
        };

        GateResult {
            lane,
            changed,
            note,
            should_stop: lane == RouteKind::Conclude,
        }
    }
}
