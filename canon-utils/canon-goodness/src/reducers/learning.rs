use canon_event::{Tick, RuntimeEvent};

use crate::Reducer;

#[derive(Default)]
pub struct Learning {
    tick: u64,
    history: Vec<(u64, bool)>, // (tick, success)
}

impl Reducer for Learning {
    fn update(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::Tick(Tick { tick, .. }) = event {
            self.tick = *tick;
        }
        if let RuntimeEvent::LoopActed(a) = event {
            self.history.push((self.tick, a.success));
            if self.history.len() > 50 {
                let keep = self.history.split_off(self.history.len() - 50);
                self.history = keep;
            }
        }
    }

    fn value(&self) -> f32 {
        let recent = self.window_rate(5);
        let base = self.window_rate(15);
        let delta = recent - base;
        (delta + 0.5).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Learning {
    fn window_rate(&self, window: u64) -> f32 {
        if self.history.is_empty() {
            return 0.5;
        }
        let start = self.tick.saturating_sub(window);
        let mut successes = 0;
        let mut total = 0;
        for (t, s) in self.history.iter().rev() {
            if *t < start {
                break;
            }
            total += 1;
            if *s {
                successes += 1;
            }
        }
        if total == 0 {
            0.5
        } else {
            successes as f32 / total as f32
        }
    }
}

