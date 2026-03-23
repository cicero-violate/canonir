use std::collections::HashMap;

use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Execution {
    planned: HashMap<String, u32>, // llm_request_id -> planned count
    completed: u32,
    total_planned: u32,
}

impl Reducer for Execution {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopPlanned(p) if p.action_kind != "no_op" => {
                if let Some(id) = p.llm_request_id.as_ref() {
                    *self.planned.entry(id.clone()).or_insert(0) += 1;
                }
                self.total_planned = self.total_planned.saturating_add(1);
            }
            RuntimeEvent::LoopActed(a) if a.success => {
                self.completed = self.completed.saturating_add(1);
                if let Some(id) = a.tool_call_id.as_ref() {
                    self.planned.remove(id);
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        if self.total_planned == 0 {
            return 1.0;
        }
        self.completed as f32 / self.total_planned as f32
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

