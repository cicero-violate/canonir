use canon_types::{EventDelta, RustcEvent, RustcState};
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

    fn handle_event(&mut self, delta: &EventDelta, _state: &RustcState) {
        if let RustcEvent::EdgeDefined { .. } = &delta.event {
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

canon_event::impl_rustc_consumer!(SmtConsumer, canon_event::EventMask::EDGE_DEFINED, handle_event);
