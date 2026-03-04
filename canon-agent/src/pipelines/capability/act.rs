use super::Delta;
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::executor_dispatch;

#[derive(Debug, Clone, Serialize)]
pub struct DeltaOutcome {
    pub delta: Delta,
    pub status: String,
    pub message: String,
}

pub fn apply_read_only(deltas: &[Delta], roots: &[PathBuf], max_output_lines: usize) -> (String, Vec<DeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;

    for delta in deltas {
        match executor_dispatch::execute_read_only(delta, roots, max_output_lines) {
            Ok((msg, out)) => {
                results.push(DeltaOutcome { delta: delta.clone(), status: "ok".into(), message: msg });
                output.push_str(out.trim_end_matches('\n'));
                output.push('\n');
            }
            Err(e) => {
                let msg = format!("ERROR: {e}");
                results.push(DeltaOutcome { delta: delta.clone(), status: "error".into(), message: msg.clone() });
                output.push_str(&msg);
                output.push('\n');
                let _ = error.get_or_insert(msg);
            }
        }
    }

    (output, results, error)
}

pub fn summarize_deltas(deltas: &[Delta]) -> Vec<serde_json::Value> {
    deltas
        .iter()
        .map(|d| match d {
            Delta::ReadFile { path } => serde_json::json!({"type":"read_file","path":path}),
            Delta::ListDir { path } => serde_json::json!({"type":"list_dir","path":path}),
            Delta::ReadCommand { command, args } => serde_json::json!({"type":"read_command","command":command,"args":args}),
            Delta::WriteFile { path, .. } => serde_json::json!({"type":"write_file","path":path}),
            Delta::ReplaceText { path, .. } => serde_json::json!({"type":"replace_text","path":path}),
            Delta::DeleteFile { path } => serde_json::json!({"type":"delete_file","path":path}),
        })
        .collect()
}

pub fn apply_mutations(
    deltas: &[Delta],
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    max_output_lines: usize,
) -> (String, Vec<DeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;

    for delta in deltas {
        match executor_dispatch::execute_mutation(delta, roots, allowed_write_roots, max_output_lines) {
            Ok(msg) => {
                results.push(DeltaOutcome { delta: delta.clone(), status: "ok".into(), message: msg.clone() });
                output.push_str(&msg);
                output.push('\n');
            }
            Err(e) => {
                let msg = format!("ERROR: {e}");
                results.push(DeltaOutcome { delta: delta.clone(), status: "error".into(), message: msg.clone() });
                output.push_str(&msg);
                output.push('\n');
                let _ = error.get_or_insert(msg);
            }
        }
    }

    (output, results, error)
}


pub(crate) fn resolve_path(path: &str, roots: &[PathBuf], allow_nonexistent: bool) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if has_parent_dir_component(&[path.to_string()]) {
        return Err(format!("path contains '..': {}", path));
    }

    let resolved = anchor(p, &roots[0]);

    if !is_within_roots(&resolved, roots) {
        return Err(format!("path escapes allowed roots: {}", resolved.display()));
    }

    if !allow_nonexistent && !resolved.exists() {
        return Err(format!("path does not exist: {}", resolved.display()));
    }

    Ok(resolved)
}

fn anchor(p: &Path, root: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { root.join(p) }
}

pub(crate) fn is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

pub(crate) fn has_parent_dir_component(args: &[String]) -> bool {
    args.iter().any(|a| a.split('/').any(|c| c == ".."))
}

pub(crate) fn truncate_lines(text: &str, max_lines: usize) -> String {
    let mut iter = text.lines();
    let kept: Vec<&str> = iter.by_ref().take(max_lines).collect();
    let remaining = iter.count();
    let mut out = kept.join("\n");
    if remaining > 0 {
        out.push_str(&format!("\n... [{} lines truncated] ...", remaining));
    }
    out
}
