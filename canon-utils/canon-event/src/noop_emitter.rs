use std::sync::Arc;

use crate::{EventEmitter, RuntimeEvent, EventId};

struct NoopEmitter;

impl EventEmitter for NoopEmitter {
    fn emit_with_parents(&self, _event: RuntimeEvent, _parents: Vec<EventId>, _file: &'static str, _line: u32) {}
}

pub fn new_noop_emitter() -> Arc<dyn EventEmitter> {
    Arc::new(NoopEmitter)
}

