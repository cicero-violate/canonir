#![feature(rustc_private)]

extern crate rustc_hir;
extern crate rustc_ast;
extern crate rustc_middle;
extern crate rustc_span;

pub mod csr;
pub mod errors;
pub mod emit;
pub mod extract;
pub mod invariant_errors;
pub mod invariants;
pub mod validator;
pub mod types;
#[cfg(feature = "canon_capture_compat")]
pub mod compat;

pub use csr::{build_csr, find_path, load_csr, CsrGraph};
pub use errors::{augment_with_errors, write_repair_surface};
pub use emit::{write_outputs, OutputConfig};
pub use extract::{extract_and_write, extract_upg, UpgGraph};
pub use types::{Edge, EdgeKind, Metadata, Node, NodeKind};
