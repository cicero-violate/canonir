use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Benefit {
    clean_verifies: u32,
    llm_calls: u32,
}

impl Reducer for Benefit {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopVerified(v) if v.compiler_clean => {
                self.clean_verifies = self.clean_verifies.saturating_add(1);
            }
            RuntimeEvent::CapabilityCompleted(c) if c.result.kind_str() == "llm.call" => {
                self.llm_calls = self.llm_calls.saturating_add(1);
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.llm_calls == 0 {
            return 1.0;
        }
        let target_ratio = 1.0 / 5.0;
        let actual_ratio = self.clean_verifies as f32 / self.llm_calls as f32;
        (actual_ratio / target_ratio).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

