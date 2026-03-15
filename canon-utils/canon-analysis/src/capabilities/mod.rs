pub mod dispatcher;
pub mod events;
pub mod graph_context;
pub mod runner;

mod run;

use canon_capability_engine::CapabilityRegistry;

pub fn register_analysis_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(run::AnalysisRunCapability));
}
