use super::{Executable, ExecutionContext, ExecutionResult};
use canon_analysis::capabilities::events::emit_analysis_event;
use canon_analysis::capabilities::runner;
use canon_event::AnalysisEvent;
use serde_json::json;
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

static ANALYSIS_WORKER_TX: std::sync::RwLock<Option<mpsc::Sender<AnalysisWork>>> = std::sync::RwLock::new(None);

pub fn init_analysis_worker() {
    let tx = spawn_analysis_worker();
    *ANALYSIS_WORKER_TX.write().unwrap() = Some(tx);
}

pub fn shutdown_analysis_worker() {
    *ANALYSIS_WORKER_TX.write().unwrap() = None;
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
                        let args = json!({ "crate": crate_name, "batch_id": work.batch_id });
                        match runner::run_full_analysis(&args) {
                            Ok(outcome) => {
                                let (status, crate_root) = match outcome {
                                    runner::RunOutcome::Ran(root) => ("complete", root),
                                    runner::RunOutcome::Skipped(root) => ("skipped", root),
                                };
                                let _ = emit_analysis_event(
                                    &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                    "analysis.completed",
                                    json!({
                                        "crate": crate_name,
                                        "status": status,
                                        "crate_root": crate_root.display().to_string(),
                                        "batch_id": batch_id,
                                    }),
                                );
                            }
                            Err(err) => {
                                let _ = emit_analysis_event(
                                    &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                    "analysis.failed",
                                    json!({
                                        "crate": crate_name,
                                        "error": err.to_string(),
                                        "batch_id": batch_id,
                                    }),
                                );
                            }
                        }
                    }
                    AnalysisWork::Workspace => match runner::run_workspace_analysis(&json!({})) {
                        Ok(outcome) => {
                            let (status, workspace_dir) = match outcome {
                                runner::RunOutcome::Ran(root) => ("complete", root),
                                runner::RunOutcome::Skipped(root) => ("skipped", root),
                            };
                            let _ = emit_analysis_event(
                                &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                "workspace.completed",
                                json!({
                                    "status": status,
                                    "workspace_dir": workspace_dir.display().to_string(),
                                }),
                            );
                        }
                        Err(err) => {
                            let _ = emit_analysis_event(
                                &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
                                "workspace.failed",
                                json!({
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

impl Executable for AnalysisEvent {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let guard = ANALYSIS_WORKER_TX.read().unwrap();
        let tx = guard.as_ref().ok_or_else(|| anyhow::anyhow!("analysis worker not initialized"))?;
        match self {
            AnalysisEvent::Run(ev) => {
                let _ = tx.send(AnalysisWork::Crate(CrateWork { crate_name: ev.crate_name, batch_id: ev.batch_id }));
            }
            AnalysisEvent::Workspace(_) => {
                let _ = tx.send(AnalysisWork::Workspace);
            }
        }
        Ok(ExecutionResult::Deferred)
    }
}
