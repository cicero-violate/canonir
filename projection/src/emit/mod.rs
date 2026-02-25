//! Emit valid Rust source from layout `Plan`s produced by `projection::layout`.
//!
//! Split from the former monolithic `emit.rs` into:
//!   - emit::emitters  (plan emitters + dispatch)
//!   - emit::body      (CFG/body emission)
//!   - emit::fmt       (formatting helpers)
//!   - emit::cargo     (Cargo.toml emitter)

mod body;
mod cargo;
mod emitters;
mod fmt;

pub use emitters::emit_plan;
