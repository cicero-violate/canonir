use std::collections::HashSet;

use canon_event::RuntimeEvent;

use crate::Reducer;

#[derive(Default)]
pub struct Love {
    clean_verifies: u64,
    total_verifies: u64,
    seen_capabilities: HashSet<String>,
    active_agents: u32,
    total_actions: u64,
    destructive_blocks: u64,
}

const TOTAL_CAPABILITIES: f32 = 6.0;

impl Reducer for Love {
    fn update(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopVerified(v) => {
                self.total_verifies = self.total_verifies.saturating_add(1);
                if v.compiler_clean {
                    self.clean_verifies = self.clean_verifies.saturating_add(1);
                }
            }
            RuntimeEvent::LoopActed(a) => {
                self.total_actions = self.total_actions.saturating_add(1);
                if matches!(a.stderr.as_str(), "rejected_destructive_command" | "blocked:destructive_command") {
                    self.destructive_blocks = self.destructive_blocks.saturating_add(1);
                }
            }
            RuntimeEvent::CapabilityCompleted(c) => {
                self.seen_capabilities.insert(c.capability.to_string());
            }
            RuntimeEvent::AgentRegistered(_) => {
                self.active_agents = self.active_agents.saturating_add(1);
            }
            _ => {}
        }
    }

    fn value(&self) -> f32 {
        let l1 = if self.total_verifies == 0 { 1.0 } else { self.clean_verifies as f32 / self.total_verifies as f32 };
        let agents = self.active_agents.max(1) as f32;
        let breadth = self.seen_capabilities.len() as f32;
        let denom = (agents * TOTAL_CAPABILITIES).min(TOTAL_CAPABILITIES);
        let l2 = if denom == 0.0 { 1.0 } else { (breadth / denom).clamp(0.0, 1.0) };
        let l3 = if self.total_actions == 0 { 1.0 } else { 1.0 - (self.destructive_blocks as f32 / self.total_actions as f32).min(1.0) };
        (l1.max(0.01) * l2.max(0.01) * l3.max(0.01)).powf(1.0 / 3.0)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}
