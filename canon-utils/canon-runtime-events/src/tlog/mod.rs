pub mod binary;
pub mod event;
pub mod rotate;
pub mod writer;

pub use binary::{is_binary_tlog, BinarySegmentWriter, BinaryTlogWriter, SegmentConfig};
pub use event::TlogEvent;
pub use rotate::{maybe_rotate, RotateConfig};
pub use writer::{emit_event_json, TlogWriter};
