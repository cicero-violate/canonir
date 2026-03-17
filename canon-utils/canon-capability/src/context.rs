use canon_event::CanonEvent;
use std::path::PathBuf;
//
#[derive(Debug, Clone)]
pub struct CapabilityExecutionContext {
    pub workspace: PathBuf,
    pub event: CanonEvent,
}
