use super::ExecutionDelta;
use serde::Serialize;
use std::path::{Path, PathBuf};
use super::executor_dispatch;
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionDeltaOutcome {
    pub delta: ExecutionDelta,
    pub status: String,
    pub message: String,
}
#[derive(Debug, Clone)]
pub struct DeltaRepairAttempt {
    pub original: ExecutionDelta,
    pub repaired: ExecutionDelta,
    pub reason: String,
}
pub fn apply_read_deltas(
    deltas: &[ExecutionDelta],
    roots: &[PathBuf],
    max_output_lines: usize,
) -> (String, Vec<ExecutionDeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;
    let mut repairs = Vec::new();
    for delta in deltas {
        let delta = repair_delta_pathing(delta, roots, &mut repairs);
        match executor_dispatch::execute_read_delta(&delta, roots, max_output_lines) {
            Ok((msg, out)) => {
                results
                    .push(ExecutionDeltaOutcome {
                        delta: delta.clone(),
                        status: "ok".into(),
                        message: msg,
                    });
                output.push_str(out.trim_end_matches('\n'));
                output.push('\n');
            }
            Err(e) => {
                let msg = format!("ERROR: {e}");
                results
                    .push(ExecutionDeltaOutcome {
                        delta: delta.clone(),
                        status: "error".into(),
                        message: msg.clone(),
                    });
                output.push_str(&msg);
                output.push('\n');
                let _ = error.get_or_insert(msg);
            }
        }
    }
    if !repairs.is_empty() {
        output.push_str("[delta_repair]\n");
        for r in repairs {
            output
                .push_str(
                    &format!(
                        "{} => {} ({})\n", format_delta_label(& r.original),
                        format_delta_label(& r.repaired), r.reason
                    ),
                );
        }
    }
    (output, results, error)
}
pub fn summarize_execution_deltas(deltas: &[ExecutionDelta]) -> Vec<serde_json::Value> {
    deltas
        .iter()
        .map(|d| match d {
            ExecutionDelta::ReadFile { path } => {
                serde_json::json!({ "type" : "read_file", "path" : path })
            }
            ExecutionDelta::ListDir { path } => {
                serde_json::json!({ "type" : "list_dir", "path" : path })
            }
            ExecutionDelta::ReadCommand { command, args } => {
                serde_json::json!(
                    { "type" : "read_command", "command" : command, "args" : args }
                )
            }
            ExecutionDelta::WriteFile { path, .. } => {
                serde_json::json!({ "type" : "write_file", "path" : path })
            }
            ExecutionDelta::ReplaceText { path, .. } => {
                serde_json::json!({ "type" : "replace_text", "path" : path })
            }
            ExecutionDelta::DeleteFile { path } => {
                serde_json::json!({ "type" : "delete_file", "path" : path })
            }
        })
        .collect()
}
pub fn apply_write_deltas(
    deltas: &[ExecutionDelta],
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    max_output_lines: usize,
) -> (String, Vec<ExecutionDeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;
    let mut repairs = Vec::new();
    for delta in deltas {
        let delta = repair_delta_pathing(delta, roots, &mut repairs);
        match executor_dispatch::execute_write_delta(
            &delta,
            roots,
            allowed_write_roots,
            max_output_lines,
        ) {
            Ok(msg) => {
                results
                    .push(ExecutionDeltaOutcome {
                        delta: delta.clone(),
                        status: "ok".into(),
                        message: msg.clone(),
                    });
                output.push_str(&msg);
                output.push('\n');
            }
            Err(e) => {
                let msg = format!("ERROR: {e}");
                results
                    .push(ExecutionDeltaOutcome {
                        delta: delta.clone(),
                        status: "error".into(),
                        message: msg.clone(),
                    });
                output.push_str(&msg);
                output.push('\n');
                let _ = error.get_or_insert(msg);
            }
        }
    }
    if !repairs.is_empty() {
        output.push_str("[delta_repair]\n");
        for r in repairs {
            output
                .push_str(
                    &format!(
                        "{} => {} ({})\n", format_delta_label(& r.original),
                        format_delta_label(& r.repaired), r.reason
                    ),
                );
        }
    }
    (output, results, error)
}
fn repair_delta_pathing(
    delta: &ExecutionDelta,
    roots: &[PathBuf],
    repairs: &mut Vec<DeltaRepairAttempt>,
) -> ExecutionDelta {
    let root = roots.get(0).cloned().unwrap_or_else(|| PathBuf::from("/"));
    match delta {
        ExecutionDelta::ReadFile { path } | ExecutionDelta::ListDir { path } => {
            let p = Path::new(path);
            if p.is_absolute() && !p.exists() {
                let repaired = match delta {
                    ExecutionDelta::ReadFile { .. } => {
                        ExecutionDelta::ReadFile {
                            path: root.display().to_string(),
                        }
                    }
                    ExecutionDelta::ListDir { .. } => {
                        ExecutionDelta::ListDir {
                            path: root.display().to_string(),
                        }
                    }
                    _ => delta.clone(),
                };
                repairs
                    .push(DeltaRepairAttempt {
                        original: delta.clone(),
                        repaired: repaired.clone(),
                        reason: "path does not exist; redirected to workspace root"
                            .to_string(),
                    });
                return repaired;
            }
            delta.clone()
        }
        _ => delta.clone(),
    }
}
fn format_delta_label(delta: &ExecutionDelta) -> String {
    match delta {
        ExecutionDelta::ReadFile { path } => format!("read_file {}", path),
        ExecutionDelta::ListDir { path } => format!("list_dir {}", path),
        ExecutionDelta::ReadCommand { command, args } => {
            format!("read_command {} {}", command, args.join(" "))
        }
        ExecutionDelta::WriteFile { path, .. } => format!("write_file {}", path),
        ExecutionDelta::ReplaceText { path, .. } => format!("replace_text {}", path),
        ExecutionDelta::DeleteFile { path } => format!("delete_file {}", path),
    }
}
pub(crate) fn resolve_delta_path(
    path: &str,
    roots: &[PathBuf],
    allow_nonexistent: bool,
) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if delta_apply_has_parent_dir_component(&[path.to_string()]) {
        return Err(format!("path contains '..': {}", path));
    }
    let resolved = anchor_path(p, &roots[0]);
    if !delta_apply_is_within_roots(&resolved, roots) {
        return Err(format!("path escapes allowed roots: {}", resolved.display()));
    }
    if !allow_nonexistent && !resolved.exists() {
        return Err(format!("path does not exist: {}", resolved.display()));
    }
    Ok(resolved)
}
fn anchor_path(p: &Path, root: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { root.join(p) }
}
pub(crate) fn delta_apply_is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}
pub(crate) fn delta_apply_has_parent_dir_component(args: &[String]) -> bool {
    args.iter().any(|a| a.split('/').any(|c| c == ".."))
}
pub(crate) fn delta_apply_truncate_lines(text: &str, max_lines: usize) -> String {
    let mut iter = text.lines();
    let kept: Vec<&str> = iter.by_ref().take(max_lines).collect();
    let remaining = iter.count();
    let mut out = kept.join("\n");
    if remaining > 0 {
        out.push_str(&format!("\n... [{} lines truncated] ...", remaining));
    }
    out
}
