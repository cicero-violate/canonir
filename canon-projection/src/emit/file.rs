use std::collections::HashSet;
use std::path::PathBuf;

use canon::ir::CanonIR;
use canon::node::CanonNodeKind;

use crate::emit::fmt::normalize_use_path;
use crate::emit::items::dispatch_item;
use crate::layout::{FilePlan, ItemPlan, Plan};

pub fn emit_plan(ir: &CanonIR, plan: &Plan) -> Vec<(PathBuf, String)> {
    plan.files.iter().map(|f| (f.path.clone(), emit_file(ir, f))).collect()
}

fn emit_file(ir: &CanonIR, file: &FilePlan) -> String {
    let mut out = String::new();
    let mut emitted_uses: HashSet<String> = HashSet::new();

    for item in &file.items {
        if let ItemPlan::Node(id) = item {
            if let CanonNodeKind::Use { path_id, .. } = &ir.node(*id).kind {
                let key = normalize_use_path(ir.lookup_path(*path_id), ir).into_owned();
                if !emitted_uses.insert(key) {
                    continue;
                }
            }
        }

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

    if out.contains("Path::") && !out.contains("use std::path::Path;") {
        out = format!("use std::path::Path;\n\n{}", out);
    }
    if out.contains("PathBuf") && !out.contains("use std::path::PathBuf;") {
        out = format!("use std::path::PathBuf;\n\n{}", out);
    }
    out
}
