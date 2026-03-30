use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Performance {
    actions_window: Vec<(u64, bool)>, // (tick, success)
    durations: Vec<u64>,
    last_tick: u64,
}

impl Reducer for Performance {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::Tick(t) => {
                self.last_tick = t.tick;
                // drop old entries beyond 10 tick window
                self.actions_window.retain(|(t, _)| self.last_tick.saturating_sub(*t) <= 10);
            }
            RuntimeEvent::LoopActed(a) => {
                self.actions_window.push((self.last_tick, a.success));
            }
            RuntimeEvent::CapabilityCompleted(c) if c.result.kind_str() == "llm.call" => {
                if let Some(ms) = c.result.duration_ms() {
                    self.durations.push(ms as u64);
                    if self.durations.len() > 100 {
                        let keep = self.durations.split_off(self.durations.len() - 100);
                        self.durations = keep;
                    }
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        let successes = self.actions_window.iter().filter(|(_, s)| *s).count() as f32;
        let window = self.actions_window.len().max(1) as f32;
        let throughput = successes / window; // successes per action slot
        let target_throughput = 0.6_f32; // heuristic

        let avg_latency = if self.durations.is_empty() { 30_000.0 } else { self.durations.iter().sum::<u64>() as f32 / self.durations.len() as f32 };
        let target_latency = 30_000.0_f32;
        let latency_factor = (target_latency / avg_latency.max(1.0)).clamp(0.0, 1.0);
        let throughput_factor = (throughput / target_throughput).clamp(0.0, 1.0);
        (throughput_factor * latency_factor).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
