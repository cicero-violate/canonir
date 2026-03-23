use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Alignment {
    score_sum: f32,
    score_count: u32,
    fallback_success: u32,
    fallback_total: u32,
}

impl Reducer for Alignment {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::RouteSelected(r) => {
                if let Some(conf) = r.confidence {
                    self.score_sum += conf;
                    self.score_count = self.score_count.saturating_add(1);
                }
            }
            RuntimeEvent::LoopActed(a) => {
                self.fallback_total = self.fallback_total.saturating_add(1);
                if a.success {
                    self.fallback_success = self.fallback_success.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.score_count > 0 {
            return (self.score_sum / self.score_count as f32).clamp(0.0, 1.0);
        }
        if self.fallback_total == 0 {
            1.0
        } else {
            self.fallback_success as f32 / self.fallback_total as f32
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
