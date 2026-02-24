use anyhow::Result;
use model::ir::model_ir::ModelIR;
use std::path::{Path, PathBuf};
use std::fs;

pub mod emit;

#[derive(Debug, Clone)]
pub struct Plan {
    pub files: Vec<(PathBuf, String)>,
}

/// Walk ModelIR and produce a Plan: one entry per Module node.
pub fn project(ir: &ModelIR) -> Result<Plan> {
    let mut files: Vec<(PathBuf, String)> = emit::emit_files(ir)
        .into_iter()
        .map(|(path, src)| (PathBuf::from(path), src))
        .collect();

    // Emit Cargo.toml using crate name and edition from NodeKind::Crate.
    // Equation:
    //   crate_node = first node where kind == Crate { name, edition }
    //   Cargo.toml = "[package]\nname = {name}\nversion = \"0.1.0\"\nedition = {edition}\n..."
    if let Some(cargo_src) = emit::emit_cargo_toml(ir) {
        files.push((PathBuf::from("Cargo.toml"), cargo_src));
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Plan { files })
}

/// Write each file in the plan to disk under `root`.
pub fn emit_to_disk(plan: &Plan, root: &Path) -> Result<()> {
    for (path, content) in &plan.files {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full, content)?;
    }
    Ok(())
}
