use canon_capability::{CapabilityExecutionContext, CapabilityRegistry};
use canon_event::CanonEvent;
use std::path::PathBuf;

/// Verify that every CanonEvent variant can be routed without panicking.
/// When a new CanonEvent variant is added, this stays up to date automatically
/// via sample_all().
pub fn assert_all_routes_safe(registry: &CapabilityRegistry) {
    for event in CanonEvent::sample_all() {
        let ctx = CapabilityExecutionContext { workspace: PathBuf::from("/tmp"), event, emitter: None };
        let result = registry.route(ctx);
        let _ = format!("{:?}", result);
    }
}
