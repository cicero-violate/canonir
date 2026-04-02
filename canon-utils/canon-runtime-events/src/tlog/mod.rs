pub mod binary;
pub mod rotate;
pub mod writer;

pub use binary::{is_binary_tlog, BinarySegmentWriter, SegmentConfig};
pub use rotate::{maybe_rotate, RotateConfig};
