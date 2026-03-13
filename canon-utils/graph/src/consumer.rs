use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};

#[derive(Debug, Default)]
pub struct GraphConsumer {
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
}

impl GraphConsumer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KernelEventConsumer for GraphConsumer {
    fn mask(&self) -> EventMask {
        EventMask::ALL
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        match &delta.event {
            KernelEvent::NodeDefined { .. } => {
                self.node_count += 1;
            }
            KernelEvent::EdgeDefined { .. } => {
                self.edge_count += 1;
            }
            KernelEvent::FileSeen { .. } => {
                self.file_count += 1;
            }
            _ => {}
        }
    }
}
