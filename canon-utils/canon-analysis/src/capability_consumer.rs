use canon_event_log::{error, info};
use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};
use std::path::PathBuf;

pub struct CapabilityEventConsumer {
    workspace: PathBuf,
    tlog_path: PathBuf,
}

impl CapabilityEventConsumer {
    pub fn new() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tlog_path = crate::capabilities::events::resolve_tlog_path();
        Self { workspace, tlog_path }
    }
}

impl KernelEventConsumer for CapabilityEventConsumer {
    fn mask(&self) -> EventMask {
        EventMask::COMPILATION_UNIT_FINISHED
    }

    fn on_event(&mut self, delta: &EventDelta, _state: &KernelState) {
        if let KernelEvent::CompilationUnitFinished { crate_name } = &delta.event {
            if let Err(err) = crate::capabilities::dispatcher::dispatch_for_event(
                &delta.event,
                &self.workspace,
                &self.tlog_path,
            ) {
                error(
                    "analysis_dispatcher",
                    "dispatch_failed",
                    serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
                );
            } else {
                info(
                    "analysis_dispatcher",
                    "dispatch_ok",
                    serde_json::json!({ "crate": crate_name }),
                );
            }
        }
    }
}
