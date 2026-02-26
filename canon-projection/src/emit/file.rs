use std::path::PathBuf;

use canon::ir::CanonIR;
use canon::node::CanonNodeKind;

use crate::emit::items::dispatch_item;
use crate::layout::{FilePlan, ItemPlan, Plan};

pub fn emit_plan(ir: &CanonIR, plan: &Plan) -> Vec<(PathBuf, String)> {
    plan.files.iter().map(|f| if f.path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") { (f.path.clone(), emit_cargo_file(ir, f)) } else { (f.path.clone(), emit_file(ir, f)) }).collect()
}

fn emit_file(ir: &CanonIR, file: &FilePlan) -> String {
    let mut out = String::new();

    for item in &file.items {
        let chunk = dispatch_item(ir, item, "");
        if chunk.is_empty() {
            continue;
        }
        out.push_str(&chunk);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }

    while out.ends_with('\n') {
        out.pop();
        if !out.ends_with('\n') {
            break;
        }
    }

    let has_path_group = out.contains("use std::path::{") && out.contains("Path");
    let has_pathbuf_group = out.contains("use std::path::{") && out.contains("PathBuf");
    if out.contains("Path::") && !out.contains("use std::path::Path;") && !has_path_group {
        out = format!("use std::path::Path;\n\n{}", out);
    }
    if out.contains("PathBuf") && !out.contains("use std::path::PathBuf;") && !has_pathbuf_group {
        out = format!("use std::path::PathBuf;\n\n{}", out);
    }
    if out.contains("symbol::") && !out.contains("use crate::symbol;") {
        out = format!("use crate::symbol;\n\n{}", out);
    }
    if out.contains("pub mod repomap;") && !out.contains("pub use crate::repomap::FileMap;") {
        out = format!("{}\npub use crate::repomap::FileMap;\n", out);
    }
    out
}

fn emit_cargo_file(ir: &CanonIR, file: &FilePlan) -> String {
    let mut out = String::new();
    for item in &file.items {
        let chunk = dispatch_item(ir, item, "");
        if chunk.is_empty() {
            continue;
        }
        out.push_str(&chunk);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}
