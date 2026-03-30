use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Correctness {
    errors: u32,
    outputs: u32,
}

impl Reducer for Correctness {
    fn update(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::LoopActed(a) = event {
            self.outputs = self.outputs.saturating_add(1);
            if !a.success && a.stderr != "skipped:batch_aborted" {
                self.errors = self.errors.saturating_add(1);
            }
        }
    }

    fn value(&self) -> f32 {
        if self.outputs == 0 {
            return 1.0;
        }
        1.0 - (self.errors as f32 / self.outputs as f32)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
