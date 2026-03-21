use anyhow::{anyhow, Result};
use canon_builder::register_build_capabilities;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_event::{CanonEvent, LlmCall};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_build_capabilities(&mut registry);

    let request_id = format!("llm-smoke-{}", std::process::id());
    let event = CanonEvent::Llm(LlmCall { request_id, prompt: "Return the JSON: {\"ok\":true}".to_string(), role: None });
    let ctx = CapabilityExecutionContext { workspace: PathBuf::from("/workspace/ai_sandbox/canon"), event, emitter: None };
    let result = registry.route(ctx)?;
    let completed = matches!(result, CapabilityExecutionResult::Emit(CanonEvent::CapabilityCompleted(_)) | CapabilityExecutionResult::EmitMany(_));
    if !completed {
        return Err(anyhow!("llm_smoke_test failed: capability did not complete"));
    }
    println!("llm_smoke_test: PASS");
    Ok(())
}
