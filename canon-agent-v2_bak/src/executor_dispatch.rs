use super::act::{delta_apply_has_parent_dir_component, delta_apply_is_within_roots, delta_apply_truncate_lines, resolve_delta_path};
use super::ExecutionDelta;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
type DeltaExecutorReadHandler = fn(&ExecutionDelta, &[PathBuf], usize) -> Result<(String, String), String>;
type DeltaExecutorWriteHandler = fn(&ExecutionDelta, &[PathBuf], &[PathBuf], usize) -> Result<String, String>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExecutionDeltaType {
    ReadFile,
    ListDir,
    ReadCommand,
    WriteFile,
    ReplaceText,
    DeleteFile,
}
static READ_EXECUTORS: Lazy<HashMap<ExecutionDeltaType, DeltaExecutorReadHandler>> = Lazy::new(|| {
    let mut map: HashMap<ExecutionDeltaType, DeltaExecutorReadHandler> = HashMap::new();
    map.insert(ExecutionDeltaType::ReadFile, apply_read_file as DeltaExecutorReadHandler);
    map.insert(ExecutionDeltaType::ListDir, apply_list_dir as DeltaExecutorReadHandler);
    map.insert(ExecutionDeltaType::ReadCommand, apply_read_command as DeltaExecutorReadHandler);
    map
});
static WRITE_EXECUTORS: Lazy<HashMap<ExecutionDeltaType, DeltaExecutorWriteHandler>> = Lazy::new(|| {
    let mut map: HashMap<ExecutionDeltaType, DeltaExecutorWriteHandler> = HashMap::new();
    map.insert(ExecutionDeltaType::WriteFile, apply_write_file as DeltaExecutorWriteHandler);
    map.insert(ExecutionDeltaType::ReplaceText, apply_replace_text as DeltaExecutorWriteHandler);
    map.insert(ExecutionDeltaType::DeleteFile, apply_delete_file as DeltaExecutorWriteHandler);
    map
});
const READONLY_COMMANDS: &[&str] = &["rg", "cat", "ls", "find", "head", "tail", "wc", "stat", "sed", "awk", "pwd", "tree"];
pub fn execute_read_delta(delta: &ExecutionDelta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let kind = delta_executor_delta_type(delta);
    let handler = READ_EXECUTORS.get(&kind).ok_or_else(|| "read_only delta type not allowed in this phase".to_string())?;
    handler(delta, roots, max_output_lines)
}
pub fn execute_write_delta(delta: &ExecutionDelta, roots: &[PathBuf], allowed_write_roots: &[PathBuf], max_output_lines: usize) -> Result<String, String> {
    let kind = delta_executor_delta_type(delta);
    let handler = WRITE_EXECUTORS.get(&kind).ok_or_else(|| "mutation delta type not allowed in this phase".to_string())?;
    handler(delta, roots, allowed_write_roots, max_output_lines)
}
fn delta_executor_delta_type(delta: &ExecutionDelta) -> ExecutionDeltaType {
    match delta {
        ExecutionDelta::ReadFile { .. } => ExecutionDeltaType::ReadFile,
        ExecutionDelta::ListDir { .. } => ExecutionDeltaType::ListDir,
        ExecutionDelta::ReadCommand { .. } => ExecutionDeltaType::ReadCommand,
        ExecutionDelta::WriteFile { .. } => ExecutionDeltaType::WriteFile,
        ExecutionDelta::ReplaceText { .. } => ExecutionDeltaType::ReplaceText,
        ExecutionDelta::DeleteFile { .. } => ExecutionDeltaType::DeleteFile,
    }
}
fn apply_read_file(delta: &ExecutionDelta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let ExecutionDelta::ReadFile { path } = delta else {
        return Err("read_file handler received wrong delta".into());
    };
    let path = resolve_delta_path(path, roots, false)?;
    if path.is_dir() {
        return apply_list_dir(&ExecutionDelta::ListDir { path: path.display().to_string() }, roots, max_output_lines);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("read_file failed for {}: {e}", path.display()))?;
    let out = format!("[read_file {}]\n{}\n", path.display(), delta_apply_truncate_lines(&content, max_output_lines));
    Ok((format!("read_file {}", path.display()), out))
}
fn apply_list_dir(delta: &ExecutionDelta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let ExecutionDelta::ListDir { path } = delta else {
        return Err("list_dir handler received wrong delta".into());
    };
    let path = resolve_delta_path(path, roots, false)?;
    let mut entries: Vec<String> =
        fs::read_dir(&path).map_err(|e| format!("list_dir failed for {}: {e}", path.display()))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    entries.sort();
    let out = format!("[list_dir {}]\n{}\n", path.display(), entries.join("\n"));
    Ok((format!("list_dir {}", path.display()), delta_apply_truncate_lines(&out, max_output_lines)))
}
fn apply_read_command(delta: &ExecutionDelta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let ExecutionDelta::ReadCommand { command, args } = delta else {
        return Err("read_command handler received wrong delta".into());
    };
    if !READONLY_COMMANDS.iter().any(|c| c == command) {
        return Err(format!("read_command rejected: {} not in whitelist", command));
    }
    if delta_apply_has_parent_dir_component(args) {
        return Err("read_command args contain '..'".into());
    }
    let output = Command::new(command).args(args).current_dir(&roots[0]).output().map_err(|e| format!("read_command failed to spawn: {e}"))?;
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
    let out = format!("[read_command {} {}]\n{}\n", command, args.join(" "), delta_apply_truncate_lines(&combined, max_output_lines));
    Ok((format!("read_command {}", command), out))
}
fn apply_write_file(delta: &ExecutionDelta, roots: &[PathBuf], allowed_write_roots: &[PathBuf], _max_output_lines: usize) -> Result<String, String> {
    let ExecutionDelta::WriteFile { path, content } = delta else {
        return Err("write_file handler received wrong delta".into());
    };
    let path = resolve_delta_path(path, roots, true)?;
    if !delta_apply_is_within_roots(&path, allowed_write_roots) {
        return Err(format!("write_file refused outside allowed roots: {}", path.display()));
    }
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
fn apply_replace_text(delta: &ExecutionDelta, roots: &[PathBuf], allowed_write_roots: &[PathBuf], _max_output_lines: usize) -> Result<String, String> {
    let ExecutionDelta::ReplaceText { path, find, replace } = delta else {
        return Err("replace_text handler received wrong delta".into());
    };
    let path = resolve_delta_path(path, roots, true)?;
    if !delta_apply_is_within_roots(&path, allowed_write_roots) {
        return Err(format!("replace_text refused outside allowed roots: {}", path.display()));
    }
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
fn apply_delete_file(delta: &ExecutionDelta, roots: &[PathBuf], allowed_write_roots: &[PathBuf], _max_output_lines: usize) -> Result<String, String> {
    let ExecutionDelta::DeleteFile { path } = delta else {
        return Err("delete_file handler received wrong delta".into());
    };
    let path = resolve_delta_path(path, roots, true)?;
    if !delta_apply_is_within_roots(&path, allowed_write_roots) {
        return Err(format!("delete_file refused outside allowed roots: {}", path.display()));
    }
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
