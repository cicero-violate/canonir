pub mod legacy;
pub mod panic_capture;
pub mod consumer;
//
pub use canon_analysis::repair::error_surface::{augment_with_errors, write_repair_surface};
pub use consumer::ReportEventConsumer;
pub use canon_analysis::invariants::invariant_validator::run_invariant_pipeline;
