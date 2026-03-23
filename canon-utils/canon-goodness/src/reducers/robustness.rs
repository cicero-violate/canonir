use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Robustness {
    prev_failed_kind: Option<String>,
    consecutive_runs: u32,
    total_fails: u32,
}

impl Reducer for Robustness {
    fn update(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::LoopActed(a) = event {
            if !a.success {
                self.total_fails = self.total_fails.saturating_add(1);
                if self.prev_failed_kind.as_deref() == Some(&a.action_kind) {
                    self.consecutive_runs = self.consecutive_runs.saturating_add(1);
                } else {
                    self.prev_failed_kind = Some(a.action_kind.clone());
                    self.consecutive_runs = 1;
                }
            } else {
                self.prev_failed_kind = None;
                self.consecutive_runs = 0;
            }
        }
    }

    fn value(&self) -> f32 {
        if self.total_fails == 0 {
            return 1.0;
        }
        1.0 - (self.consecutive_runs as f32 / self.total_fails as f32).min(1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

