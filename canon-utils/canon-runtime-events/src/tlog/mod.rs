pub mod binary;
pub mod event;
pub mod rotate;
pub mod writer;

pub use binary::{is_binary_tlog, BinarySegmentWriter, SegmentConfig};
pub use rotate::{maybe_rotate, RotateConfig};
pub(crate) use writer::emit_canon_event_json;
