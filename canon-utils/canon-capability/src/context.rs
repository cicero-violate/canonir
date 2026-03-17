use canon_event::{CanonEvent, EventEmitterHandle};
use std::path::PathBuf;

#[derive(Clone)]
pub struct CapabilityExecutionContext {
    pub workspace: PathBuf,
    pub event: CanonEvent,
    pub emitter: Option<EventEmitterHandle>,
}
