use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use once_cell::sync::Lazy;

use super::Delta;
use super::act::{has_parent_dir_component, is_within_roots, resolve_path, truncate_lines};

type ReadHandler = fn(&Delta, &[PathBuf], usize) -> Result<(String, String), String>;
type WriteHandler = fn(&Delta, &[PathBuf], &[PathBuf], usize) -> Result<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DeltaType {
    ReadFile,
    ListDir,
    ReadCommand,
    WriteFile,
    ReplaceText,
    DeleteFile,
}

static READ_EXECUTORS: Lazy<HashMap<DeltaType, ReadHandler>> = Lazy::new(|| {
    let mut map: HashMap<DeltaType, ReadHandler> = HashMap::new();
    map.insert(DeltaType::ReadFile, handle_read_file as ReadHandler);
    map.insert(DeltaType::ListDir, handle_list_dir as ReadHandler);
    map.insert(DeltaType::ReadCommand, handle_read_command as ReadHandler);
    map
});

static WRITE_EXECUTORS: Lazy<HashMap<DeltaType, WriteHandler>> = Lazy::new(|| {
    let mut map: HashMap<DeltaType, WriteHandler> = HashMap::new();
    map.insert(DeltaType::WriteFile, handle_write_file as WriteHandler);
    map.insert(DeltaType::ReplaceText, handle_replace_text as WriteHandler);
    map.insert(DeltaType::DeleteFile, handle_delete_file as WriteHandler);
    map
});

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

pub fn execute_read_only(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let kind = delta_type(delta);
    let handler = READ_EXECUTORS
        .get(&kind)
        .ok_or_else(|| "read_only delta type not allowed in this phase".to_string())?;
    handler(delta, roots, max_output_lines)
}

pub fn execute_mutation(
    delta: &Delta,
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    max_output_lines: usize,
) -> Result<String, String> {
    let kind = delta_type(delta);
    let handler = WRITE_EXECUTORS
        .get(&kind)
        .ok_or_else(|| "mutation delta type not allowed in this phase".to_string())?;
    handler(delta, roots, allowed_write_roots, max_output_lines)
}

fn delta_type(delta: &Delta) -> DeltaType {
    match delta {
        Delta::ReadFile { .. } => DeltaType::ReadFile,
        Delta::ListDir { .. } => DeltaType::ListDir,
        Delta::ReadCommand { .. } => DeltaType::ReadCommand,
        Delta::WriteFile { .. } => DeltaType::WriteFile,
        Delta::ReplaceText { .. } => DeltaType::ReplaceText,
        Delta::DeleteFile { .. } => DeltaType::DeleteFile,
    }
}

fn handle_read_file(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let Delta::ReadFile { path } = delta else {
        return Err("read_file handler received wrong delta".into());
    };
    let path = resolve_path(path, roots, false)?;
    if path.is_dir() {
        return handle_list_dir(&Delta::ListDir { path: path.display().to_string() }, roots, max_output_lines);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("read_file failed for {}: {e}", path.display()))?;
    let out = format!("[read_file {}]\n{}\n", path.display(), truncate_lines(&content, max_output_lines));
    Ok((format!("read_file {}", path.display()), out))
}

fn handle_list_dir(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let Delta::ListDir { path } = delta else {
        return Err("list_dir handler received wrong delta".into());
    };
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

fn handle_read_command(delta: &Delta, roots: &[PathBuf], max_output_lines: usize) -> Result<(String, String), String> {
    let Delta::ReadCommand { command, args } = delta else {
        return Err("read_command handler received wrong delta".into());
    };
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

fn handle_write_file(
    delta: &Delta,
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    _max_output_lines: usize,
) -> Result<String, String> {
    let Delta::WriteFile { path, content } = delta else {
        return Err("write_file handler received wrong delta".into());
    };
    let path = resolve_path(path, roots, true)?;
    if !is_within_roots(&path, allowed_write_roots) {
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

fn handle_replace_text(
    delta: &Delta,
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    _max_output_lines: usize,
) -> Result<String, String> {
    let Delta::ReplaceText { path, find, replace } = delta else {
        return Err("replace_text handler received wrong delta".into());
    };
    let path = resolve_path(path, roots, true)?;
    if !is_within_roots(&path, allowed_write_roots) {
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

fn handle_delete_file(
    delta: &Delta,
    roots: &[PathBuf],
    allowed_write_roots: &[PathBuf],
    _max_output_lines: usize,
) -> Result<String, String> {
    let Delta::DeleteFile { path } = delta else {
        return Err("delete_file handler received wrong delta".into());
    };
    let path = resolve_path(path, roots, true)?;
    if !is_within_roots(&path, allowed_write_roots) {
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
