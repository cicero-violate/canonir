use std::collections::{HashMap, HashSet};

use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};

#[derive(Debug, Default)]
pub struct QueryConsumer {
    pub symbols: HashMap<String, String>,
    pub files: HashSet<String>,
}

impl QueryConsumer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KernelEventConsumer for QueryConsumer {
    fn mask(&self) -> EventMask {
        EventMask::NODE_DEFINED | EventMask::FILE_SEEN
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        match &delta.op {
            KernelEvent::NodeDefined { symbol, kind } => {
                self.symbols.insert(symbol.clone(), kind.clone());
            }
            KernelEvent::FileSeen { path } => {
                self.files.insert(path.clone());
            }
            KernelEvent::EdgeDefined { .. } => {}
        }
    }
}
