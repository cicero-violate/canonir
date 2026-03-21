use anyhow::{anyhow, Result};
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_editor::register_editor_capabilities;
use canon_event::{CanonEvent, CapabilityRequested};

fn is_valid_event(event: &CanonEvent) -> Result<()> {
    // minimal invariant: must be matchable + printable
    match event {
        CanonEvent::Edit(_) => {}
        _ => {}
    }

    let _ = format!("{:?}", event); // must not panic
    Ok(())
}

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_editor_capabilities(&mut registry);

    // ⚠️ no registry.list() → must define capabilities explicitly OR track at registration time
    let capabilities = vec![
        canon_editor::CAP_RENAME_SYMBOL,
        // add others here OR expose via registry later
    ];

    if capabilities.is_empty() {
        return Err(anyhow!("no capabilities available"));
    }

    let base_req = CapabilityRequested { request_id: "invariant-smoke".to_string(), name: "".to_string(), args: serde_json::json!({}) };

    let mut executed = 0usize;

    for cap in capabilities {
        let mut req = base_req.clone();
        req.name = cap.to_string();

        let ctx = CapabilityExecutionContext { workspace: "/workspace/ai_sandbox/canon".into(), event: CanonEvent::CapabilityRequested(req.clone()), emitter: None };

        let result = registry.execute(&req.name, ctx)?;

        executed += 1;

        match result {
            CapabilityExecutionResult::Emit(event) => {
                is_valid_event(&event)?;
            }
            _ => {
                // accept all other variants (forward compatible)
            }
        }
    }

    if executed == 0 {
        return Err(anyhow!("no capabilities executed"));
    }

    println!("capability_invariant_test: PASS (executed={})", executed);
    Ok(())
}
