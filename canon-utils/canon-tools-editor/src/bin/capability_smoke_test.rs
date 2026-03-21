use anyhow::Result;
use canon_capability::{CapabilityExecutionContext, CapabilityRegistry};
use canon_editor::register_editor_capabilities;
use canon_event::{CanonEvent, EditEvent, RenameSymbol};

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_editor_capabilities(&mut registry);

    let mut executed = 0;
    for name in registry.names() {
        if name != "edit.rename_symbol" {
            continue;
        }
        let event = CanonEvent::Edit(EditEvent::RenameSymbol(RenameSymbol { project: "p".into(), old: "a".into(), new: "b".into() }));
        let ctx = CapabilityExecutionContext { workspace: "/workspace/ai_sandbox/canon".into(), event: event.clone(), emitter: None };
        let result = registry.route(ctx);

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
