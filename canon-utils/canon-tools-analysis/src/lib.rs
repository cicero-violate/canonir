pub mod analysis;
pub mod query;
pub use query::{QueryConsumer, QueryOptions, QueryError, TlogQueryResult, query_file, query_file_single};
// supervisor trigger: no-op change
pub mod invariants;
pub mod semantics;
pub mod repair;
pub mod report_pipeline;
pub mod report_consumer;
pub mod panic_capture;
pub mod workspace;
pub mod capabilities;
pub mod capability_consumer;
mod report_types;
mod panic_types;

pub use invariants::invariant_validator::run_invariant_pipeline;
pub use repair::error_surface::{augment_with_errors, write_repair_surface};
pub use report_pipeline::{generate_reports, generate_reports_from_tlog};
pub use report_consumer::ReportEventConsumer;
pub use capability_consumer::CapabilityEventConsumer;
pub use capabilities::register_analysis_capabilities;
pub use workspace::aggregator::aggregate_workspace;
pub use workspace::layout_verify::verify_reports_layout;
pub use workspace::migrate::migrate_reports_layout;
pub use report_types::*;
pub use panic_types::PanicRecord;
pub mod smt;

pub use smt::consumer::SmtConsumer;
