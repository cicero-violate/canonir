use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler};
use canon_event::{AnalysisEvent, AnalysisRun, AnalysisWorkspace, CanonEvent};
use std::sync::mpsc;
use std::thread;

struct CrateWork {
    crate_name: String,
    batch_id: Option<String>,
}

enum AnalysisWork {
    Crate(CrateWork),
    Workspace,
}

fn spawn_analysis_worker() -> mpsc::Sender<AnalysisWork> {
    let (tx, rx) = mpsc::channel::<AnalysisWork>();
    thread::Builder::new()
        .name("analysis_worker".to_string())
        .spawn(move || {
            for work in rx {
                match work {
                    AnalysisWork::Crate(work) => {
                        let crate_name = work.crate_name.clone();
                        let batch_id = work.batch_id.clone().unwrap_or_default();
                        let args = serde_json::json!({ "crate": crate_name, "batch_id": work.batch_id });
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
                    AnalysisWork::Workspace => match crate::capabilities::runner::run_workspace_analysis(&serde_json::json!({})) {
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

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Analysis(AnalysisEvent::Run(AnalysisRun { request_id: _, crate_name, batch_id })) => {
                let _ = self.work_tx.send(AnalysisWork::Crate(CrateWork { crate_name, batch_id }));
                Ok(CapabilityExecutionResult::Deferred)
            }
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

impl CapabilityHandler for AnalysisWorkspaceCapability {
    fn name(&self) -> &'static str {
        "analysis.workspace"
    }

    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        match ctx.event {
            CanonEvent::Analysis(AnalysisEvent::Workspace(AnalysisWorkspace { .. })) => {
                let _ = self.work_tx.send(AnalysisWork::Workspace);
                Ok(CapabilityExecutionResult::Deferred)
            }
            _ => Ok(CapabilityExecutionResult::NoOp),
        }
    }
}

pub fn new_analysis_capabilities() -> (AnalysisRunCapability, AnalysisWorkspaceCapability) {
    let tx = spawn_analysis_worker();
    (AnalysisRunCapability::new(tx.clone()), AnalysisWorkspaceCapability::new(tx))
}
