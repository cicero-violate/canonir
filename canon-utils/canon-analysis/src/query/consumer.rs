use std::collections::{HashMap, HashSet};

use canon_event::emit_debug::info;
use canon_event::{EventMask, RustcEventConsumer};
use canon_types::{EventDelta, RustcEvent, RustcState};
use serde_json::json;

#[derive(Debug, Default)]
pub struct QueryConsumer {
    pub symbols: HashMap<String, String>,
    pub files: HashSet<String>,
    logged: bool,
}

impl QueryConsumer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RustcEventConsumer for QueryConsumer {
    fn mask(&self) -> EventMask {
        EventMask::NODE_DEFINED | EventMask::FILE_SEEN
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &RustcState) {
        match &delta.event {
            RustcEvent::NodeDefined { symbol, kind, .. } => {
                self.symbols.insert(symbol.clone(), kind.clone());
            }
            RustcEvent::FileSeen { path } => {
                self.files.insert(path.clone());
            }
            RustcEvent::EdgeDefined { .. } => {}
            _ => {}
        }
        if !self.logged && (!self.symbols.is_empty() || !self.files.is_empty()) {
            self.logged = true;
            info(
                "query_consumer",
                "event_stream_started",
                json!({
                    "symbols": self.symbols.len(),
                    "files": self.files.len()
                }),
            );
        }
    }
}
