use canon_event::emit_debug::info;
use canon_event::{EventMask, RustcEventConsumer};
use canon_types::{EventDelta, RustcEvent, RustcState};
use serde_json::json;

#[derive(Debug, Default)]
pub struct GraphConsumer {
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    logged: bool,
}

impl GraphConsumer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RustcEventConsumer for GraphConsumer {
    fn mask(&self) -> EventMask {
        EventMask::ALL
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &RustcState) {
        match &delta.event {
            RustcEvent::NodeDefined { .. } => {
                self.node_count += 1;
            }
            RustcEvent::EdgeDefined { .. } => {
                self.edge_count += 1;
            }
            RustcEvent::FileSeen { .. } => {
                self.file_count += 1;
            }
            _ => {}
        }
        if !self.logged && (self.node_count + self.edge_count + self.file_count) > 0 {
            self.logged = true;
            info(
                "graph_consumer",
                "event_stream_started",
                json!({
                    "nodes": self.node_count,
                    "edges": self.edge_count,
                    "files": self.file_count
                }),
            );
        }
    }
}
