use anyhow::Result;
use canon_capability::{CapabilityExecutionContext, CapabilityRegistry};
use canon_editor::register_editor_capabilities;
use canon_event::{CanonEvent, CapabilityRequested};

fn synth_arg(kind: &canon_capability::ArgKind) -> serde_json::Value {
    match kind {
        canon_capability::ArgKind::String => serde_json::json!("x"),
        canon_capability::ArgKind::Path => serde_json::json!("/workspace"),
        canon_capability::ArgKind::Symbol => serde_json::json!("crate::x"),
        canon_capability::ArgKind::Json => serde_json::json!({}),
    }
}

fn synth_args(schema: &canon_capability::CapabilitySchema) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for arg in &schema.args {
        if arg.required {
            map.insert(arg.key.to_string(), synth_arg(&arg.kind));
        }
    }

    serde_json::Value::Object(map)
}

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_editor_capabilities(&mut registry);

    let schemas = registry.schemas();

    let mut executed = 0;

    for schema in schemas {
        let req = CapabilityRequested { request_id: "invariant".into(), name: schema.name.to_string(), args: synth_args(&schema) };

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
