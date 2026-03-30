pub mod analysis;
pub mod capabilities;
pub mod graph_artifacts;
pub mod query;
pub use query::{query_file, query_file_single, QueryConsumer, QueryError, QueryOptions, TlogQueryResult};
// supervisor trigger: no-op change
pub mod infer_schema_event;
pub mod invariants;
pub mod llm_report;
pub mod panic_capture;
mod panic_types;
pub mod repair;
pub mod report_pipeline;
mod report_types;
pub mod semantics;
pub mod workspace;

pub use graph_artifacts::{
    duplicate_definition_rename_candidates, graph_backed_module_hotspots, graph_backed_module_moves, graph_backed_rename_candidates, graph_import_bindings, latest_graph_artifact_path,
    load_graph_artifact, load_latest_workspace_graph_artifact, module_cohesion_hotspots, resolve_graph_symbol_path, verify_graph_expectations, GraphArtifactIndex, GraphArtifactSummary,
    GraphImportBinding, GraphModuleMoveCandidate, GraphProofExpectation, GraphProofReport, GraphRenameCandidate, GraphResolvedSymbol, ModuleCohesionHotspot,
};
pub use invariants::invariant_validator::run_invariant_pipeline;
pub use panic_types::PanicRecord;
pub use repair::error_surface::{augment_with_errors, write_repair_surface};
pub use report_pipeline::{generate_reports, generate_reports_for_crate, generate_reports_from_tlog};
pub use report_types::*;
pub use workspace::aggregator::aggregate_workspace;
pub use workspace::layout_verify::verify_reports_layout;
pub use workspace::migrate::migrate_reports_layout;
pub mod smt;

pub use smt::consumer::SmtConsumer;
