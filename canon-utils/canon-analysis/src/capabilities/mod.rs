pub mod dispatcher;
pub mod events;
pub mod graph_context;
pub mod runner;

mod dead_code;
mod dependency_cycles;
mod structural_hotspots;
mod callgraph_centrality;
mod dataflow_fanout;
mod invariants;
mod smt;
mod repair_surface;
mod semantic_clusters;

use canon_capability::CapabilityRegistry;

pub fn register_analysis_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(dead_code::DeadCodeCapability));
    registry.register(std::sync::Arc::new(dependency_cycles::DependencyCyclesCapability));
    registry.register(std::sync::Arc::new(structural_hotspots::StructuralHotspotsCapability));
    registry.register(std::sync::Arc::new(callgraph_centrality::CallgraphCentralityCapability));
    registry.register(std::sync::Arc::new(dataflow_fanout::DataflowFanoutCapability));
    registry.register(std::sync::Arc::new(invariants::InvariantPipelineCapability));
    registry.register(std::sync::Arc::new(smt::SmtInvariantCapability));
    registry.register(std::sync::Arc::new(repair_surface::RepairSurfaceCapability));
    registry.register(std::sync::Arc::new(semantic_clusters::SemanticClusteringCapability));
}
