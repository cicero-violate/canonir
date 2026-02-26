use anyhow::Result;
use canon::ir::CanonIR;
use std::fs;
use std::path::{Path, PathBuf};

pub mod emit;
pub mod layout;

pub use layout::{FilePlan, ItemPlan, Plan};

pub fn project(ir: &CanonIR) -> Result<Plan> {
    layout::build_plan(ir)
}

pub fn emit(ir: &CanonIR, plan: &Plan) -> Vec<(PathBuf, String)> {
    emit::emit_plan(ir, plan)
}

pub fn emit_to_disk(ir: &CanonIR, plan: &Plan, root: &Path) -> Result<()> {
    for (path, content) in emit(ir, plan) {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full, content)?;
    }
    Ok(())
}
