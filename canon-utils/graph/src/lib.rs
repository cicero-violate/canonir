pub mod consumer;
pub mod csr;

pub use canon_types::{Node, Edge, NodeKind, EdgeKind, SpanRange};
pub use csr::{CsrGraph, build_csr, find_path, load_csr};
pub use consumer::GraphConsumer;
