use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{BashInvoke, RuntimeEvent, CapabilityCompleted, CapabilityFailed, CapabilityResult, EventEmitterHandle, ProcessResult};
use std::process::Command;

struct BashWork {
    request_id: String,
    cmd: String,
    cwd: String,
    emitter: EventEmitterHandle,
    trigger_id: canon_event::EventId,
}

static BASH_WORKER_TX: std::sync::RwLock<Option<std::sync::mpsc::Sender<BashWork>>> =
    std::sync::RwLock::new(None);

pub fn init_bash_worker() {
    let (tx, rx) = std::sync::mpsc::channel::<BashWork>();
    *BASH_WORKER_TX.write().unwrap() = Some(tx);

    std::thread::Builder::new()
        .name("bash_executor_worker".to_string())
        .spawn(move || {
            for BashWork { request_id, cmd, cwd, emitter, trigger_id } in rx {
                std::fs::create_dir_all(&cwd).ok();
                let result = Command::new("bash")
                    .arg("-lc")
                    .arg(&cmd)
                    .current_dir(&cwd)
                    .output();
                match result {
                    Ok(output) => {
                        emitter.emit_child(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                            request_id,
                            capability: "bash",
                            result: CapabilityResult::Process(ProcessResult {
                                status: output.status.code().unwrap_or(-1),
                                success: output.status.success(),
                                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                            }),
                        }), vec![trigger_id.clone()], file!(), line!());
                    }
                    Err(err) => {
                        emitter.emit_child(RuntimeEvent::ErrorOccurred(canon_event::new_error_occurred(
                            "bash_execution_failed",
                            "bash_executor",
                            err.to_string(),
                            "error",
                            serde_json::json!({
                                "request_id": request_id.clone(),
                                "capability": "bash"
                            }),
                            Some(request_id.clone()),
                        )), vec![trigger_id.clone()], file!(), line!());
                        emitter.emit_child(RuntimeEvent::CapabilityFailed(CapabilityFailed {
                            request_id,
                            capability: "bash",
                            error: err.to_string(),
                        }), vec![trigger_id.clone()], file!(), line!());
                    }
                }
            }
        })
        .expect("bash worker thread spawn failed");
}

pub fn shutdown_bash_worker() {
    *BASH_WORKER_TX.write().unwrap() = None;
}

impl Executable for BashInvoke {
    fn execute(self, ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.unwrap_or_else(|| ".".to_string());
        if let Some(tx) = BASH_WORKER_TX.read().unwrap().as_ref() {
            let _ = tx.send(BashWork {
                request_id: self.request_id,
                cmd: self.cmd,
                cwd,
                emitter: ctx.emitter,
                trigger_id: ctx.trigger_id,
            });
            Ok(ExecutionResult::Deferred)
        } else {
            // Fallback inline execution if worker is not initialized.
            std::fs::create_dir_all(&cwd).ok();
            let output = Command::new("bash").arg("-lc").arg(&self.cmd).current_dir(&cwd).output()?;
            Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                request_id: self.request_id,
                capability: "bash",
                result: CapabilityResult::Process(ProcessResult {
                    status: output.status.code().unwrap_or(-1),
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                }),
            })))
        }
    }
}
