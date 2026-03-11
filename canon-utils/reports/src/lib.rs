mod reports;
mod errors;
pub mod consumer;

pub use reports::generate_reports;
pub use reports::generate_reports_from_tlog;
pub use errors::{augment_with_errors, write_repair_surface};
pub use consumer::ReportConsumer;
