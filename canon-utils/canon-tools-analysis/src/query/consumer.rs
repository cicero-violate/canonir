use std::collections::{HashMap, HashSet};

use canon_types::{EventDelta, RustcEvent, RustcState};

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
            RustcEvent::NodeDefined(canon_types::NodeDefined { symbol, kind, .. }) => {
                self.symbols.insert(symbol.clone(), kind.clone());
            }
            RustcEvent::FileSeen(canon_types::FileSeen { path }) => {
                self.files.insert(path.clone());
            }
            RustcEvent::EdgeDefined(_) => {}
            _ => {}
        }
        if !self.logged && (!self.symbols.is_empty() || !self.files.is_empty()) {
            self.logged = true;
        }
    }
}

canon_event::impl_rustc_consumer!(
    QueryConsumer,
    canon_event::EventMask::NODE_DEFINED | canon_event::EventMask::FILE_SEEN,
    handle_event
);
