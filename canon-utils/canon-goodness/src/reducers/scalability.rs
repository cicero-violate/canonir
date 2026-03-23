use canon_runtime_events::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Scalability {
    first_window: Vec<bool>,
    recent_window: Vec<bool>,
    tick: u64,
}

impl Reducer for Scalability {
    fn update(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::RouteTick(t) = event {
            self.tick = t.tick;
        }
        if let RuntimeEvent::LoopActed(a) = event {
            if self.tick < 10 {
                if self.first_window.len() < 10 {
                    self.first_window.push(a.success);
                }
            } else {
                if self.recent_window.len() >= 10 {
                    self.recent_window.remove(0);
                }
                self.recent_window.push(a.success);
            }
        }
    }

    fn value(&self) -> f32 {
        if self.first_window.len() < 3 || self.recent_window.len() < 3 {
            return 1.0;
        }
        let base = self.first_window.iter().filter(|s| **s).count() as f32 / self.first_window.len() as f32;
        let recent = self.recent_window.iter().filter(|s| **s).count() as f32 / self.recent_window.len() as f32;
        if base == 0.0 {
            return 1.0;
        }
        (recent / base).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

