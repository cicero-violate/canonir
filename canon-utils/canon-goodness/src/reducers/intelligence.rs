use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Intelligence {
    llm_calls: u32,
    useful_llm_calls: u32,
}

impl Reducer for Intelligence {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::CapabilityCompleted(c) if c.result.kind_str() == "llm.call" => {
                self.llm_calls = self.llm_calls.saturating_add(1);
            }
            RuntimeEvent::LoopPlanned(p) => {
                if p.action_kind != "no_op" && p.action_kind != "error" {
                    self.useful_llm_calls = self.useful_llm_calls.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.llm_calls == 0 {
            return 1.0;
        }
        self.useful_llm_calls as f32 / self.llm_calls as f32
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
