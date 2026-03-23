use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Scalability {
    completed: u32,
    active_agents: u32,
    ticks: u64,
    target_rate: f32, // successes per agent per tick
}

impl Scalability {
    pub fn new() -> Self {
        Self { target_rate: 1.0 / 5.0, ..Default::default() }
    }
}

impl Reducer for Scalability {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::AgentRegistered(_) => {
                self.active_agents = self.active_agents.saturating_add(1);
            }
            RuntimeEvent::LoopActed(a) if a.success => {
                self.completed = self.completed.saturating_add(1);
            }
            RuntimeEvent::Tick(t) => {
                self.ticks = t.tick;
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        let agents = self.active_agents.max(1) as f32;
        let ticks = self.ticks.max(1) as f32;
        let rate = self.completed as f32 / (agents * ticks);
        (rate / self.target_rate).clamp(0.0, 1.0)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}
