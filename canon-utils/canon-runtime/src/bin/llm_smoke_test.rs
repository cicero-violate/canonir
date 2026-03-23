use anyhow::{anyhow, Result};
use canon_event::{RuntimeEvent, LlmCall};
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use std::path::PathBuf;

struct NoopEmitter;
impl canon_event::EventEmitter for NoopEmitter {
    fn emit(&self, _event: RuntimeEvent) {}
    fn emit_located(&self, _event: RuntimeEvent, _file: &'static str, _line: u32) {}
}

fn main() -> Result<()> {
    let event = RuntimeEvent::Llm(LlmCall {
        request_id: format!("llm-smoke-{}", std::process::id()),
        prompt: "Return the JSON: {\"ok\":true}".to_string(),
        role: None,
        agent_id: None,
    });
    let exec = ExecutableEvent::try_from(event).expect("llm call should be executable");
    let ctx = ExecutionContext { workspace: PathBuf::from("/workspace/ai_sandbox/canon"), emitter: std::sync::Arc::new(NoopEmitter) };
    let result = exec.execute(ctx)?;
    match result {
        ExecutionResult::Deferred => {
            println!("llm_smoke_test: PASS (deferred to worker)");
            Ok(())
        }
        ExecutionResult::Emit(RuntimeEvent::CapabilityCompleted(_)) | ExecutionResult::EmitMany(_) => Ok(()),
        other => Err(anyhow!("unexpected result: {:?}", other)),
    }
}
