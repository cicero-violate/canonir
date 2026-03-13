use canon_types::RuntimeEvent;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CapabilityContext {
    pub workspace: PathBuf,
    pub event: RuntimeEvent,
}
