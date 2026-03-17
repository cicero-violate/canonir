use canon_types::{EventDelta, RustcEvent, RustcState};

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
        if let RustcEvent::EdgeDefined(_) = &delta.event {
            self.edge_count += 1;
            if !self.logged {
                self.logged = true;
            }
        }
    }
}

canon_event::impl_rustc_consumer!(SmtConsumer, canon_event::EventMask::EDGE_DEFINED, handle_event);
