// CONTRACT:
// - No sorting
// - No graph traversal
// - No mutation
// - Pure string rendering of Plan

use std::path::PathBuf;

use crate::emit::items::dispatch_item;
use crate::layout::{FilePlan, Plan};

/// Emit the full plan into `(path, source)` pairs.
pub fn emit_plan(plan: &Plan) -> Vec<(PathBuf, String)> {
    plan.files.iter().map(|f| (f.path.clone(), emit_file(f))).collect()
}

fn emit_file(file: &FilePlan) -> String {
    let mut out = String::new();
    for item in &file.items {
        out.push_str(&dispatch_item(item, ""));
        if !out.ends_with('\n') {
            out.push('\n');
        }
        // Separate top-level items with a newline for readability.
        out.push('\n');
    }
    // Trim trailing whitespace newline added after the last item.
    while out.ends_with('\n') {
        out.pop();
        if !out.ends_with('\n') {
            break;
        }
    }
    out
}
