use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Efficiency {
    useful_actions: u32,
    llm_calls: u32,
    total_duration_ms: u64,
}

impl Reducer for Efficiency {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::CapabilityCompleted(c) if c.result.kind_str() == "llm.call" => {
                self.llm_calls = self.llm_calls.saturating_add(1);
                if let Some(ms) = c.result.duration_ms() {
                    self.total_duration_ms = self.total_duration_ms.saturating_add(ms as u64);
                }
            }
            RuntimeEvent::LoopActed(a) if a.success => {
                self.useful_actions = self.useful_actions.saturating_add(1);
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.llm_calls == 0 {
            return 1.0;
        }
        let avg_duration = self.total_duration_ms as f32 / self.llm_calls as f32;
        let target = 30_000.0_f32;
        let duration_factor = (target / avg_duration.max(1.0)).clamp(0.0, 1.0);
        let productivity = self.useful_actions as f32 / self.llm_calls as f32;
        (productivity * duration_factor).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

