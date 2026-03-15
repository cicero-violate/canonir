use canon_event::{EventMask, KernelEventConsumer};
use canon_types::{EventDelta, KernelEvent, KernelState};
use canon_event::emit_debug::info;
use serde_json::json;

#[derive(Debug, Default)]
pub struct SmtConsumer {
    pub edge_count: usize,
    logged: bool,
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
            if !self.logged {
                self.logged = true;
                info(
                    "smt_consumer",
                    "edge_stream_started",
                    json!({ "edge_count": self.edge_count }),
                );
            }
        }
    }
}
