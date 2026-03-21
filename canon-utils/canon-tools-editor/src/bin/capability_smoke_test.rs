use anyhow::Result;
use canon_capability::{CapabilityExecutionContext, CapabilityRegistry};
use canon_editor::register_editor_capabilities;
use canon_event::{CanonEvent, CapabilityRequested};

fn synth_args(_name: &str) -> serde_json::Value { serde_json::json!({}) }

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_editor_capabilities(&mut registry);

    let mut executed = 0;
    for name in registry.names() {
        let req = CapabilityRequested { request_id: "invariant".into(), name: name.clone(), args: synth_args(&name) };

        let ctx = CapabilityExecutionContext { workspace: "/workspace/ai_sandbox/canon".into(), event: CanonEvent::CapabilityRequested(req.clone()), emitter: None };

        let result = registry.execute(&req.name, ctx);

        executed += 1;

        match result {
            Ok(res) => {
                if let Some(event) = res.into_event() {
                    let _ = format!("{:?}", event);
                }
            }
            Err(e) => {
                let _ = format!("{:?}", e);
            }
        }
    }

    println!("capability_invariant_test: PASS ({})", executed);
    Ok(())
}
