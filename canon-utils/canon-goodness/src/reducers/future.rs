use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct FutureProof {
    last_verify_clean: bool,
    regressions: u32,
    changes: u32,
}

impl Reducer for FutureProof {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopVerified(v) => {
                self.last_verify_clean = v.compiler_clean;
            }
            RuntimeEvent::LoopActed(a) => {
                let is_change = matches!(a.action_kind.as_str(), "apply_patch" | "write_file" | "run_command");
                if is_change && a.success {
                    self.changes = self.changes.saturating_add(1);
                }
                if !a.success && self.last_verify_clean {
                    self.regressions = self.regressions.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.changes == 0 {
            return 1.0;
        }
        1.0 - (self.regressions as f32 / self.changes as f32).min(1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
