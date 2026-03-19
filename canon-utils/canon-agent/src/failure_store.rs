use canon_event::{CanonEvent, RustcEvent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailureStoreFailureStats {
    pub total: usize,
    pub cycle: usize,
    pub deadlock: usize,
    pub failure_pattern_rate: f64,
    pub cycle_frequency: f64,
    pub deadlock_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailureStore {
    stats: FailureStoreFailureStats,
}

impl FailureStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::ErrorOccurred(payload) => {
                self.record_message(&payload.kind, &payload.message);
            }
            CanonEvent::CapabilityFailed(payload) => {
                self.record_message("capability_failed", &payload.error);
            }
            CanonEvent::NodeFailed(payload) => {
                let message = payload.error.as_deref().unwrap_or("node_failed");
                self.record_message("node_failed", message);
            }
            CanonEvent::Code(code) => match &code.delta.event {
                RustcEvent::PanicCaptured(payload) => {
                    self.record_message("panic_captured", &payload.message);
                }
                RustcEvent::InvariantViolation(payload) => {
                    self.record_message("invariant_violation", &payload.message);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub fn stats(&self) -> &FailureStoreFailureStats {
        &self.stats
    }

    fn record_message(&mut self, kind: &str, message: &str) {
        self.stats.total += 1;
        let is_cycle = kind.contains("cycle") || message.to_lowercase().contains("cycle");
        let is_deadlock = kind.contains("deadlock") || message.to_lowercase().contains("deadlock");
        if is_cycle {
            self.stats.cycle += 1;
        }
        if is_deadlock {
            self.stats.deadlock += 1;
        }
        let total = self.stats.total.max(1) as f64;
        let cycle = self.stats.cycle as f64;
        let deadlock = self.stats.deadlock as f64;
        self.stats.failure_pattern_rate = (cycle + deadlock) / total;
        self.stats.cycle_frequency = cycle / total;
        self.stats.deadlock_rate = deadlock / total;
    }
}
