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
