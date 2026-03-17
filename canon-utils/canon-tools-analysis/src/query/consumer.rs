use std::collections::{HashMap, HashSet};

use canon_event::emit_debug::info;
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

    fn handle_event(&mut self, delta: &EventDelta, _state: &RustcState) {
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

canon_event::impl_rustc_consumer!(
    QueryConsumer,
    canon_event::EventMask::NODE_DEFINED | canon_event::EventMask::FILE_SEEN,
    handle_event
);
