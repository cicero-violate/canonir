pub mod events;
pub mod graph_context;
pub mod runner;

mod run;

use canon_capability::CapabilityRegistry;

pub fn register_analysis_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(run::AnalysisRunCapability));
    registry.register(std::sync::Arc::new(run::AnalysisWorkspaceCapability));
}
