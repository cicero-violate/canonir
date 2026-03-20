use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler};
use canon_event::CanonEvent;
use std::sync::mpsc;
use std::thread;

enum AnalysisWork {
    Crate(serde_json::Value),
    Workspace(serde_json::Value),
}

fn spawn_analysis_worker() -> mpsc::Sender<AnalysisWork> {
    let (tx, rx) = mpsc::channel::<AnalysisWork>();
    thread::Builder::new()
        .name("analysis_worker".to_string())
        .spawn(move || {
            for work in rx {
                match work {
                    AnalysisWork::Crate(args) => {
                        let crate_name = args.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        let batch_id = args.get("batch_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        match crate::capabilities::runner::run_full_analysis(&args) {
                            Ok(outcome) => {
                                let (status, crate_root) = match outcome {
                                    crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
                                    crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
                                };
                                let _ = crate::capabilities::events::emit_analysis_event(
                                    &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                    "analysis.completed",
                                    serde_json::json!({
                                        "crate": crate_name,
                                        "status": status,
                                        "crate_root": crate_root.display().to_string(),
                                        "batch_id": batch_id,
                                    }),
                                );
                            }
                            Err(err) => {
                                let _ = crate::capabilities::events::emit_analysis_event(
                                    &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                    "analysis.failed",
                                    serde_json::json!({
                                        "crate": crate_name,
                                        "error": err.to_string(),
                                        "batch_id": batch_id,
                                    }),
                                );
                            }
                        }
                    }
                    AnalysisWork::Workspace(args) => match crate::capabilities::runner::run_workspace_analysis(&args) {
                        Ok(outcome) => {
                            let (status, workspace_dir) = match outcome {
                                crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
                                crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
                            };
                            let _ = crate::capabilities::events::emit_analysis_event(
                                &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                "workspace.completed",
                                serde_json::json!({
                                    "status": status,
                                    "workspace_dir": workspace_dir.display().to_string(),
                                }),
                            );
                        }
                        Err(err) => {
                            let _ = crate::capabilities::events::emit_analysis_event(
                                &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                "workspace.failed",
                                serde_json::json!({
                                    "error": err.to_string(),
                                }),
                            );
                        }
                    },
                }
            }
        })
        .expect("analysis worker thread");
    tx
}

pub struct AnalysisRunCapability {
    work_tx: mpsc::Sender<AnalysisWork>,
}

pub struct AnalysisWorkspaceCapability {
    work_tx: mpsc::Sender<AnalysisWork>,
}

impl AnalysisRunCapability {
    fn new(work_tx: mpsc::Sender<AnalysisWork>) -> Self {
        Self { work_tx }
    }
}

impl AnalysisWorkspaceCapability {
    fn new(work_tx: mpsc::Sender<AnalysisWork>) -> Self {
        Self { work_tx }
    }
}

impl CapabilityHandler for AnalysisRunCapability {
    fn name(&self) -> &'static str {
        "analysis.run"
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let _ = self.work_tx.send(AnalysisWork::Crate(request.args));
        Ok(CapabilityExecutionResult::Deferred)
    }
}

impl CapabilityHandler for AnalysisWorkspaceCapability {
    fn name(&self) -> &'static str {
        "analysis.workspace"
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let _ = self.work_tx.send(AnalysisWork::Workspace(request.args));
        Ok(CapabilityExecutionResult::Deferred)
    }
}

pub fn new_analysis_capabilities() -> (AnalysisRunCapability, AnalysisWorkspaceCapability) {
    let tx = spawn_analysis_worker();
    (AnalysisRunCapability::new(tx.clone()), AnalysisWorkspaceCapability::new(tx))
}
