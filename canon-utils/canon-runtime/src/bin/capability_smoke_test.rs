use anyhow::{anyhow, Result};
use canon_builder::register_build_capabilities;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_event::{CanonEvent, FileEvent, FileRead};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_build_capabilities(&mut registry);

    let event = CanonEvent::File(FileEvent::Read(FileRead { request_id: "capability-smoke-read".to_string(), path: "/workspace/ai_sandbox/canon/canon-utils/README.md".to_string() }));
    let ctx = CapabilityExecutionContext { workspace: PathBuf::from("/workspace/ai_sandbox/canon"), event, emitter: None };
    let result = registry.route(ctx)?;
    let completed = matches!(result, CapabilityExecutionResult::Emit(CanonEvent::CapabilityCompleted(_)) | CapabilityExecutionResult::EmitMany(_));
    if !completed {
        return Err(anyhow!("capability_smoke_test failed: capability did not complete"));
    }
    println!("capability_smoke_test: PASS");
    Ok(())
}
