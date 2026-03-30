use anyhow::{anyhow, Result};
use canon_event::{EventId, FileEvent, FileRead, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use std::path::PathBuf;

struct NoopEmitter;
impl canon_event::EventEmitter for NoopEmitter {
    fn emit_with_parents(&self, _event: RuntimeEvent, _parents: Vec<canon_event::EventId>, _file: &'static str, _line: u32) {}
}

fn main() -> Result<()> {
    let event = RuntimeEvent::File(FileEvent::Read(FileRead { request_id: "capability-smoke-read".to_string(), path: "/workspace/ai_sandbox/canon/canon-utils/README.md".to_string(), queued: true }));
    let exec = ExecutableEvent::try_from(event).expect("file read should be executable");
    let ctx = ExecutionContext { workspace: PathBuf::from("/workspace/ai_sandbox/canon"), emitter: std::sync::Arc::new(NoopEmitter), trigger_id: EventId::new("root") };
    let result = exec.execute(ctx)?;
    let completed = matches!(result, ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(_)) | ExecutionResult::EmitMany(_));
    if !completed {
        return Err(anyhow!("capability_smoke_test failed: did not emit CapabilityCompleted"));
    }
    println!("capability_smoke_test: PASS");
    Ok(())
}
