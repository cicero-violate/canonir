#[cfg(test)]
mod tests {
    use crate::registry::CapabilityRegistry;
    use canon_capability::CapabilityExecutionContext;
    use canon_event::{CanonEvent, CapabilityRequested};
    use std::path::PathBuf;

    fn sample_capability_events() -> Vec<CanonEvent> {
        vec![
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-1".to_string(),
                name: "edit.rename_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "old": "foo", "new": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-2".to_string(),
                name: "edit.move_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "symbol": "foo", "module": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-3".to_string(),
                name: "edit.delete_symbol".to_string(),
                args: serde_json::json!({ "project": "p", "symbol": "foo" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-4".to_string(),
                name: "edit.rename_module".to_string(),
                args: serde_json::json!({ "project": "p", "old": "foo", "new": "bar" }),
            }),
            CanonEvent::CapabilityRequested(CapabilityRequested {
                request_id: "test-5".to_string(),
                name: "edit.rename_dir".to_string(),
                args: serde_json::json!({ "project": "p", "old": "src/a", "new": "src/b" }),
            }),
        ]
    }

    #[test]
    fn all_capability_routes_are_safe() {
        use canon_tools_editor::capabilities::register_editor_capabilities;

        let mut registry = CapabilityRegistry::new();
        register_editor_capabilities(&mut registry);

        for event in sample_capability_events() {
            let ctx = CapabilityExecutionContext { workspace: PathBuf::from("/tmp"), event, emitter: None };
            let result = registry.route(ctx);
            let _ = format!("{:?}", result);
        }
    }
}
