use crate::structured::SymbolKind;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn determine_source_root(project: &Path) -> PathBuf {
    let src = project.join("src");
    if src.is_dir() {
        src
    } else {
        project.to_path_buf()
    }
}

pub(crate) fn module_path_from_file(root: &Path, file: &Path) -> Result<String> {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let mut components: Vec<String> = rel.components().filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string())).collect();
    if components.is_empty() {
        return Err(anyhow!("cannot derive module path for {}", file.display()));
    }
    let filename = components.pop().unwrap();
    let module_segments = if filename == "lib.rs" || filename == "main.rs" {
        components
    } else if filename == "mod.rs" {
        components
    } else {
        let stem = filename.trim_end_matches(".rs").to_string();
        let mut segs = components;
        segs.push(stem);
        segs
    };
    let mut path = String::from("crate");
    for segment in module_segments {
        if !segment.is_empty() {
            path.push_str("::");
            path.push_str(&segment);
        }
    }
    Ok(path)
}

pub(crate) fn module_path_for_dir(root: &Path, dir: &Path) -> Result<Vec<String>> {
    let rel = dir.strip_prefix(root).with_context(|| format!("directory {} is not under {}", dir.display(), root.display()))?;
    let mut segments: Vec<String> = vec!["crate".to_string()];
    for component in rel.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if !s.is_empty() {
                segments.push(s.to_string());
            }
        }
    }
    Ok(segments)
}

pub(crate) fn canonicalize_relative(path: &Path, root: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(root.join(path))
    }
}

pub(crate) fn split_module_segments(path: &str) -> Vec<&str> {
    path.split("::").filter(|s| !s.is_empty()).collect()
}

pub(crate) fn split_module_path(path: &str) -> Vec<String> {
    split_module_segments(path).into_iter().map(|s| s.to_string()).collect()
}

pub(crate) fn join_module_path(segments: &[String]) -> String {
    segments.join("::")
}

pub(crate) fn normalize_module_path(module_path: &str, uses_crate_prefix: bool) -> String {
    if uses_crate_prefix {
        if module_path == "crate" || module_path.starts_with("crate::") {
            return module_path.to_string();
        }
        return format!("crate::{module_path}");
    }
    module_path
        .strip_prefix("crate::")
        .unwrap_or(module_path)
        .to_string()
}

pub(crate) fn infer_crate_name(project_root: &Path) -> Option<String> {
    let manifest = project_root.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(manifest) else {
        return None;
    };
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let value = rest.trim();
                if let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                    return Some(stripped.replace('-', "_"));
                }
            }
        }
    }
    None
}

pub(crate) fn build_full_path(module_path: &str, name: &str) -> Vec<String> {
    let mut segments = split_module_path(module_path);
    segments.push(name.to_string());
    segments
}

pub(crate) fn symbol_kind_from_str(kind: &str) -> SymbolKind {
    match kind {
        "fn" => SymbolKind::Fn,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "const" => SymbolKind::Const,
        "static" => SymbolKind::Static,
        "type" => SymbolKind::Type,
        "trait" => SymbolKind::Trait,
        "module" => SymbolKind::Module,
        _ => SymbolKind::Fn,
    }
}
