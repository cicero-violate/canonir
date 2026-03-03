//! Delta execution for the multi-dag pipeline.
//!
//! - Read-only deltas are for analysis; mutation deltas are for execution.
//! - No free-form shell: commands are explicit and whitelisted.

use super::Delta;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const READONLY_COMMANDS: &[&str] = &[
    "rg",
    "cat",
    "ls",
    "find",
    "head",
    "tail",
    "wc",
    "stat",
    "sed",
    "awk",
    "pwd",
    "tree",
];

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
        match execute_read_only(delta, roots, max_output_lines) {
            Ok((msg, out)) => {
                results.push(DeltaOutcome { delta: delta.clone(), status: "ok".into(), message: msg });
                if !out.trim().is_empty() {
                    output.push_str(&out);
                    if !out.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
            Err(e) => {
                let msg = format!("ERROR: {e}");
                results.push(DeltaOutcome { delta: delta.clone(), status: "error".into(), message: msg.clone() });
                output.push_str(&msg);
                output.push('\n');
                if error.is_none() {
                    error = Some(msg);
                }
            }
        }
    }

    (output, results, error)
}

pub fn apply_mutations(deltas: &[Delta], roots: &[PathBuf], max_output_lines: usize) -> (String, Vec<DeltaOutcome>, Option<String>) {
    let mut output = String::new();
    let mut results = Vec::new();
    let mut error: Option<String> = None;

    for delta in deltas {
        match execute_mutation(delta, roots, max_output_lines) {
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
                if error.is_none() {
                    error = Some(msg);
                }
            }
        }
    }

    (output, results, error)
}

fn execute_read_only(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    match delta {
        Delta::ReadFile { path } => {
            let path = resolve_path(path, roots, false)?;
            let content = fs::read_to_string(&path).map_err(|e| format!("read_file failed for {}: {e}", path.display()))?;
            let out = format!("[read_file {}]\n{}\n", path.display(), truncate_lines(&content, max_output_lines));
            Ok((format!("read_file {}", path.display()), out))
        }
        Delta::ListDir { path } => {
            let path = resolve_path(path, roots, false)?;
            let mut entries: Vec<String> = fs::read_dir(&path)
                .map_err(|e| format!("list_dir failed for {}: {e}", path.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            entries.sort();
            let out = format!("[list_dir {}]\n{}\n", path.display(), entries.join("\n"));
            Ok((format!("list_dir {}", path.display()), truncate_lines(&out, max_output_lines)))
        }
        Delta::ReadCommand { command, args } => {
            if !READONLY_COMMANDS.iter().any(|c| c == command) {
                return Err(format!("read_command rejected: {} not in whitelist", command));
            }
            if has_parent_dir_component(args) {
                return Err("read_command args contain '..'".into());
            }
            let output = Command::new(command)
                .args(args)
                .current_dir(&roots[0])
                .output()
                .map_err(|e| format!("read_command failed to spawn: {e}"))?;
            let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str("--- stderr ---\n");
                combined.push_str(&stderr);
            }
            let code = output.status.code().unwrap_or(1);
            if code != 0 {
                combined.push_str(&format!("\n[exit code {}]", code));
            }
            let out = format!("[read_command {} {}]\n{}\n", command, args.join(" "), truncate_lines(&combined, max_output_lines));
            Ok((format!("read_command {}", command), out))
        }
        _ => Err("read_only delta type not allowed in this phase".into()),
    }
}

fn execute_mutation(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<String, String> {
    match delta {
        Delta::WriteFile { path, content } => {
            let path = resolve_path(path, roots, true)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("write_file failed to create parent dirs: {e}"))?;
            }
            let existing = fs::read_to_string(&path).unwrap_or_default();
            if existing == *content {
                return Ok(format!("write_file {} (no-op)", path.display()));
            }
            fs::write(&path, content).map_err(|e| format!("write_file failed for {}: {e}", path.display()))?;
            Ok(format!("write_file {} ({} bytes)", path.display(), content.len()))
        }
        Delta::ReplaceText { path, find, replace } => {
            let path = resolve_path(path, roots, true)?;
            let content = fs::read_to_string(&path).map_err(|e| format!("replace_text failed to read {}: {e}", path.display()))?;
            let occurrences = content.match_indices(find).count();
            if occurrences == 0 {
                if content.contains(replace) {
                    return Ok(format!("replace_text {} (no-op, already replaced)", path.display()));
                }
                return Err(format!("replace_text did not find target in {}", path.display()));
            }
            let updated = content.replace(find, replace);
            fs::write(&path, updated).map_err(|e| format!("replace_text failed to write {}: {e}", path.display()))?;
            Ok(format!("replace_text {} ({} replacements)", path.display(), occurrences))
        }
        Delta::DeleteFile { path } => {
            let path = resolve_path(path, roots, true)?;
            if path.is_dir() {
                return Err(format!("delete_file refused to remove directory {}", path.display()));
            }
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("delete_file failed for {}: {e}", path.display()))?;
                Ok(format!("delete_file {} (removed)", path.display()))
            } else {
                Ok(format!("delete_file {} (no-op)", path.display()))
            }
        }
        _ => Err("mutation delta type not allowed in this phase".into()),
    }
}

fn resolve_path(path: &str, roots: &[PathBuf], allow_nonexistent: bool) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if has_parent_dir_component(&[path.to_string()]) {
        return Err(format!("path contains '..': {}", path));
    }

    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        roots[0].join(p)
    };

    if !is_within_roots(&resolved, roots) {
        return Err(format!("path escapes allowed roots: {}", resolved.display()));
    }

    if !allow_nonexistent && !resolved.exists() {
        return Err(format!("path does not exist: {}", resolved.display()));
    }

    Ok(resolved)
}

fn is_within_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

fn has_parent_dir_component(args: &[String]) -> bool {
    args.iter().any(|a| a.split('/').any(|c| c == ".."))
}

fn truncate_lines(text: &str, max_lines: usize) -> String {
    let mut lines = text.lines();
    let mut kept = Vec::with_capacity(max_lines);
    let mut count = 0usize;
    for line in lines.by_ref() {
        if count >= max_lines {
            break;
        }
        kept.push(line);
        count += 1;
    }
    let remaining = lines.count();
    let mut out = kept.join("\n");
    if remaining > 0 {
        out.push_str(&format!("\n... [{} lines truncated] ...", remaining));
    }
    out
}
