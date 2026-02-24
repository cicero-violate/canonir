#![feature(rustc_private)]

extern crate model;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

pub mod ast;
pub mod cargo_project;
pub mod driver;
pub mod hir;
pub mod mir;

use ast::capture_ast;
use cargo_project::CargoProject;
use driver::{run_hir, run_mir};
use model::model_ir::Model;

use std::path::Path;

/// Unified entry point:
/// - Resolve Cargo project
/// - Build dependencies
/// - Inject extern flags
/// - Run HIR + MIR passes
pub fn run(entry: &Path) -> Result<Model, Box<dyn std::error::Error>> {
    let entry = entry.canonicalize()?;

    // --- AST pre-pass (syn-based) ---
    let mut model = capture_ast(&entry)?;
    println!("AST modules captured: {}", model.modules.len());
    println!("AST functions captured: {}", model.functions.len());

    let project = CargoProject::from_entry(&entry)?;

    project.ensure_dependencies_built()?;

    let targets = project.targets()?;

    let _primary = targets.iter().find(|t| t.kind.iter().any(|k| k == "lib")).or_else(|| targets.first()).ok_or("no cargo target found")?;

    // Use Cargo's *exact* rustc invocation (includes cfg/features/proc-macros/externs/out-dir/etc).
    let args = project.rustc_args_from_cargo_verbose()?;

    let hir_model = run_hir(&args);
    let mir_model = run_mir(&args);

    println!("HIR items captured: {}", hir_model.hir_items.len());
    println!("MIR bodies captured: {}", mir_model.mir_bodies.len());

    model.hir_items = hir_model.hir_items;
    model.hir_exprs = hir_model.hir_exprs;
    model.hir_types = hir_model.hir_types;
    model.hir_paths = hir_model.hir_paths;
    model.mir_bodies = mir_model.mir_bodies;

    Ok(model)
}

// Deleted duplicate API.
// `run()` is the single public entry point.

pub fn write_model_json(model: &Model, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, model)?;
    Ok(())
}
