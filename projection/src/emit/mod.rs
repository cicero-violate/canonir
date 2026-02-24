//! Emit valid Rust source from ModelIR.
//!
//! Split from the former monolithic `emit.rs` into:
//!   - emit::emitters  (node-kind emitters + dispatch)
//!   - emit::body      (CFG/body emission)
//!   - emit::fmt       (formatting helpers)
//!   - emit::cargo     (Cargo.toml emitter)

mod body;
mod cargo;
mod emitters;
mod fmt;

pub use cargo::emit_cargo_toml;
pub use emitters::{emit_files, emit_node};
