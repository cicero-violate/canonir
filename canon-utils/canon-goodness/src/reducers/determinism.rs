use std::collections::HashMap;

use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Determinism {
    per_kind: HashMap<String, (u32, u32)>, // kind -> (successes, attempts)
}

impl Reducer for Determinism {
    fn update(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::LoopActed(a) = event {
            let entry = self.per_kind.entry(a.action_kind.clone()).or_insert((0, 0));
            entry.1 = entry.1.saturating_add(1);
            if a.success {
                entry.0 = entry.0.saturating_add(1);
            }
        }
    }

    fn value(&self) -> f32 {
        if self.per_kind.len() <= 1 {
            return 1.0;
        }
        let mut rates = Vec::new();
        for (_k, (succ, att)) in self.per_kind.iter() {
            if *att == 0 {
                continue;
            }
            rates.push(*succ as f32 / *att as f32);
        }
        if rates.is_empty() {
            return 1.0;
        }
        let mean = rates.iter().sum::<f32>() / rates.len() as f32;
        let var = rates.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / rates.len() as f32;
        (1.0 - (var * 4.0)).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

