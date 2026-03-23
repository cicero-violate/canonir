use std::fs;
use std::path::{Path, PathBuf};

pub fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    if let Some(val) = args
        .windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
    {
        return Some(val);
    }
    let prefix = format!("{flag}=");
    args.iter()
        .find_map(|arg| arg.strip_prefix(&prefix).map(|v| v.to_string()))
}

pub fn find_flag_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values: Vec<String> = args
        .windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .collect();
    let prefix = format!("{flag}=");
    for arg in args {
        if let Some(val) = arg.strip_prefix(&prefix) {
            values.push(val.to_string());
        }
    }
    values
}

pub fn project_root_from_out_dir(args: &[String]) -> Option<PathBuf> {
    let out_dir = find_flag_value(args, "--out-dir").map(PathBuf::from)?;
    project_root_from_target_path(&out_dir)
}

pub fn graph_output_dir(args: &[String]) -> PathBuf {
    let start = project_root_from_out_dir(args)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let workspace_root = find_workspace_root(&start).unwrap_or(start);
    workspace_root.join("state").join("rustc")
}

pub fn project_root_from_target_path(out_dir: &Path) -> Option<PathBuf> {
    let mut cursor = Some(out_dir);
    while let Some(path) = cursor {
        if path.file_name().and_then(|s| s.to_str()) == Some("target") {
            return path.parent().map(|p| p.to_path_buf());
        }
        cursor = path.parent();
    }
    None
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let manifest = dir.join("Cargo.toml");
        if let Ok(text) = fs::read_to_string(&manifest) {
            if text.contains("[workspace]") {
                return Some(dir.to_path_buf());
            }
        }
    }
    None
}

pub fn workspace_root_from_output_dir(output_dir: &Path) -> PathBuf {
    find_workspace_root(output_dir)
        .or_else(|| output_dir.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| output_dir.to_path_buf())
}

pub fn is_cargo_registry_path(path: &Path) -> bool {
    if path
        .components()
        .any(|c| c.as_os_str() == "registry" || c.as_os_str() == "git")
        && path.components().any(|c| c.as_os_str() == ".cargo")
    {
        return true;
    }
    let raw = path.to_string_lossy();
    raw.contains("/.cargo/registry/") || raw.contains("/.cargo/git/")
}
