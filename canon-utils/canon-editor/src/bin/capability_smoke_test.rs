use anyhow::{anyhow, Result};
use canon_capability_engine::{CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_editor::register_editor_capabilities;
use canon_event::{CapabilityRequested, RuntimeEvent};

fn main() -> Result<()> {
    let mut registry = CapabilityRegistry::new();
    register_editor_capabilities(&mut registry);

    let request = CapabilityRequested {
        request_id: "editor-smoke-1".to_string(),
        name: canon_editor::CAP_RENAME_SYMBOL.to_string(),
        args: serde_json::json!({
            "project": "/workspace/ai_sandbox/canon",
            "old": "crate::dummy",
            "new": "dummy_renamed"
        }),
    };

    let ctx = CapabilityContext {
        workspace: "/workspace/ai_sandbox/canon".into(),
        event: RuntimeEvent::CapabilityRequested(request.clone()),
    };

    let result = registry.execute(&request.name, ctx)?;
    let event = match result {
        CapabilityResult::Emit(event) => event,
        other => {
            return Err(anyhow!(
                "unexpected capability result: expected Emit(Edit), got {:?}",
                other
            ));
        }
    };
    let RuntimeEvent::Edit(edit) = event else {
        return Err(anyhow!("unexpected runtime event variant"));
    };

    match edit {
        canon_event::EditEvent::RenameSymbol { project, old, new } => {
            if project != "/workspace/ai_sandbox/canon" || old != "crate::dummy" || new != "dummy_renamed" {
                return Err(anyhow!("rename symbol event mismatch"));
            }
        }
        _ => return Err(anyhow!("unexpected edit event type")),
    }

    println!("capability_smoke_test: PASS");
    Ok(())
}
