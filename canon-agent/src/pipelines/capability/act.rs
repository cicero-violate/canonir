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

#[derive(Debug, Clone)]
pub struct DeltaRepairLog {
    pub original: Delta,
    pub repaired: Delta,
    pub reason: String,
}

pub fn apply_read_only(deltas: &[Delta], roots: &[PathBuf], max_output_lines: usize) -> (String, Vec<DeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;
    let mut repairs = Vec::new();

    for delta in deltas {
        let delta = repair_delta_path(delta, roots, &mut repairs);
        match executor_dispatch::execute_read_only(&delta, roots, max_output_lines) {
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
    if !repairs.is_empty() {
        output.push_str("[delta_repair]\n");
        for r in repairs {
            output.push_str(&format!("{} => {} ({})\n", delta_label(&r.original), delta_label(&r.repaired), r.reason));
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
    let mut repairs = Vec::new();

    for delta in deltas {
        let delta = repair_delta_path(delta, roots, &mut repairs);
        match executor_dispatch::execute_mutation(&delta, roots, allowed_write_roots, max_output_lines) {
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
    if !repairs.is_empty() {
        output.push_str("[delta_repair]\n");
        for r in repairs {
            output.push_str(&format!("{} => {} ({})\n", delta_label(&r.original), delta_label(&r.repaired), r.reason));
        }
    }

    (output, results, error)
}

fn repair_delta_path(delta: &Delta, roots: &[PathBuf], repairs: &mut Vec<DeltaRepairLog>) -> Delta {
    let root = roots.get(0).cloned().unwrap_or_else(|| PathBuf::from("/"));
    match delta {
        Delta::ReadFile { path } | Delta::ListDir { path } => {
            let p = Path::new(path);
            if p.is_absolute() && !p.exists() {
                let repaired = match delta {
                    Delta::ReadFile { .. } => Delta::ReadFile { path: root.display().to_string() },
                    Delta::ListDir { .. } => Delta::ListDir { path: root.display().to_string() },
                    _ => delta.clone(),
                };
                repairs.push(DeltaRepairLog {
                    original: delta.clone(),
                    repaired: repaired.clone(),
                    reason: "path does not exist; redirected to workspace root".to_string(),
                });
                return repaired;
            }
            delta.clone()
        }
        _ => delta.clone(),
    }
}

fn delta_label(delta: &Delta) -> String {
    match delta {
        Delta::ReadFile { path } => format!("read_file {}", path),
        Delta::ListDir { path } => format!("list_dir {}", path),
        Delta::ReadCommand { command, args } => format!("read_command {} {}", command, args.join(" ")),
        Delta::WriteFile { path, .. } => format!("write_file {}", path),
        Delta::ReplaceText { path, .. } => format!("replace_text {}", path),
        Delta::DeleteFile { path } => format!("delete_file {}", path),
    }
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
