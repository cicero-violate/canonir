use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{RuntimeEvent, CapabilityCompleted, CapabilityResult, FileEvent, ProcessResult};

impl Executable for FileEvent {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            FileEvent::Read(ev) => {
                let content = std::fs::read_to_string(&ev.path)?;
                Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.read",
                    result: CapabilityResult::Process(ProcessResult { status: 0, success: true, stdout: content, stderr: String::new() }),
                })))
            }
            FileEvent::Write(ev) => {
                std::fs::write(&ev.path, &ev.content)?;
                Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.write",
                    result: CapabilityResult::Empty,
                })))
            }
            FileEvent::Patch(ev) => {
                let content = std::fs::read_to_string(&ev.path)?;
                let patched = content.replace(&ev.old, &ev.new);
                std::fs::write(&ev.path, patched)?;
                Ok(ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                    request_id: ev.request_id,
                    capability: "file.patch",
                    result: CapabilityResult::Empty,
                })))
            }
        }
    }
}
