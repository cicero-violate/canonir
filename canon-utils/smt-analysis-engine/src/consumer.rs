use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};

#[derive(Debug, Default)]
pub struct SmtConsumer {
    pub edge_count: usize,
}

impl SmtConsumer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KernelEventConsumer for SmtConsumer {
    fn mask(&self) -> EventMask {
        EventMask::EDGE_DEFINED
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        if let KernelEvent::EdgeDefined { .. } = &delta.event {
            self.edge_count += 1;
        }
    }
}
