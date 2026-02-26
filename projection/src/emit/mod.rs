//! Emit valid Rust source from layout `Plan`s produced by `projection::layout`.
//!
//! Split from the former monolithic `emit.rs` into:
//!   - emit::file      (plan emitters + dispatch)
//!   - emit::items     (module/file item dispatch)
//!   - emit::functions (fn/trait emitters)
//!   - emit::impls     (impl emitters)
//!   - emit::types     (struct/enum/type emitters)
//!   - emit::macros    (macro call shim emitter)
//!   - emit::body      (CFG/body emission)
//!   - emit::fmt       (formatting helpers)
//!   - emit::cargo     (Cargo.toml emitter)

mod body;
mod cargo;
mod file;
mod fmt;
mod functions;
mod helpers;
mod impls;
mod items;
mod macros;
mod types;

pub use file::emit_plan;
