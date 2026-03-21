use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{BashInvoke, RuntimeEvent, CapabilityCompleted, CapabilityResult, ProcessResult};
use std::process::Command;

impl Executable for BashInvoke {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        let cwd = self.cwd.clone().unwrap_or_else(|| ".".to_string());
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
