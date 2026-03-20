pub mod events;
pub mod graph_context;
pub mod runner;

mod run;

use canon_capability::CapabilityRegistry;

pub fn register_analysis_capabilities(registry: &mut CapabilityRegistry) {
    let (run_cap, workspace_cap) = run::new_analysis_capabilities();
    registry.register(std::sync::Arc::new(run_cap));
    registry.register(std::sync::Arc::new(workspace_cap));
}
