use canon_event::emit_debug::{error, info};
use canon_types::{EventDelta, RustcEvent, RustcState};
use std::path::PathBuf;

pub struct CapabilityEventConsumer {
    workspace: PathBuf,
    tlog_path: PathBuf,
    reports_root: Option<PathBuf>,
}

impl CapabilityEventConsumer {
    pub fn new() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tlog_path = canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG"));
        let reports_root = std::env::var("CANON_REPORTS_OUT").ok().map(PathBuf::from);
        Self {
            workspace,
            tlog_path,
            reports_root,
        }
    }

    fn handle_event(&mut self, delta: &EventDelta, _state: &RustcState) {
        if let RustcEvent::CompilationUnitFinished { crate_name } = &delta.event {
            match crate::capabilities::dispatcher::dispatch_for_event(
                &delta.event,
                &self.workspace,
                &self.tlog_path,
            ) {
                Ok(batch_id) => {
                    info(
                        "analysis_dispatcher",
                        "dispatch_ok",
                        serde_json::json!({ "crate": crate_name, "batch_id": batch_id }),
                    );
                    if !batch_id.is_empty() {
                        let reports_root = self
                            .reports_root
                            .clone()
                            .unwrap_or_else(|| self.workspace.join("state").join("reports_out"));
                        let args = serde_json::json!({
                            "crate": crate_name,
                            "batch_id": batch_id,
                            "workspace": self.workspace.display().to_string(),
                            "reports_root": reports_root.display().to_string()
                        });
                        let start = std::time::Instant::now();
                        match crate::capabilities::runner::run_full_analysis(&args) {
                            Ok(outcome) => {
                                let (status, crate_root) = match outcome {
                                    crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
                                    crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
                                };
                                info(
                                    "analysis_dispatcher",
                                    "analysis_run_complete",
                                    serde_json::json!({
                                        "crate": crate_name,
                                        "status": status,
                                        "elapsed_ms": start.elapsed().as_millis()
                                    }),
                                );
                                if let Err(err) = crate::capabilities::events::emit_analysis_event(
                                    &self.tlog_path,
                                    "analysis.completed",
                                    serde_json::json!({
                                        "crate": crate_name,
                                        "status": status,
                                        "crate_root": crate_root.display().to_string(),
                                        "batch_id": batch_id
                                    }),
                                ) {
                                    error(
                                        "analysis_dispatcher",
                                        "emit_analysis_completed_failed",
                                        serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
                                    );
                                }
                            }
                            Err(err) => {
                                error(
                                    "analysis_dispatcher",
                                    "analysis_run_failed",
                                    serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
                                );
                                if let Err(err) = crate::capabilities::events::emit_analysis_event(
                                    &self.tlog_path,
                                    "analysis.failed",
                                    serde_json::json!({
                                        "crate": crate_name,
                                        "error": err.to_string(),
                                        "batch_id": batch_id
                                    }),
                                ) {
                                    error(
                                        "analysis_dispatcher",
                                        "emit_analysis_failed_failed",
                                        serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
                                    );
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    error(
                        "analysis_dispatcher",
                        "dispatch_failed",
                        serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
                    );
                }
            }
        }
    }
}

canon_event::impl_rustc_consumer!(
    CapabilityEventConsumer,
    canon_event::EventMask::COMPILATION_UNIT_FINISHED,
    handle_event
);
