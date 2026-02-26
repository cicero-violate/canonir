use anyhow::Result;
use model::ir::model_ir::ModelIR;
use std::fs;
use std::path::{Path, PathBuf};

pub mod emit;
pub mod layout;

pub use layout::{FilePlan, ItemPlan, Plan};

/// Walk ModelIR and produce a layout Plan (no source strings).
#[deprecated(note = "Model projection is legacy-only. Prefer canon-projection on CanonIR.")]
pub fn project(ir: &ModelIR) -> Result<Plan> {
    layout::build_plan(ir)
}

/// Convert a layout Plan into concrete `(path, source)` pairs.
pub fn emit(plan: &Plan) -> Vec<(PathBuf, String)> {
    emit::emit_plan(plan)
}

/// Write each emitted file to disk under `root`.
pub fn emit_to_disk(plan: &Plan, root: &Path) -> Result<()> {
    for (path, content) in emit(plan) {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full, content)?;
    }
    Ok(())
}
