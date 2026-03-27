use canon_goal::parse_agent_goal_markdown;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntrypointKind {
    None,
    Bin,
    Lib,
    Mixed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleGap {
    pub declared_in: PathBuf,
    pub module_name: String,
    pub expected_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceModel {
    pub target_root: PathBuf,
    pub path_exists: bool,
    pub repo_initialized: bool,
    pub cargo_toml_exists: bool,
    pub cargo_lock_exists: bool,
    pub crate_name: Option<String>,
    pub src_dir_exists: bool,
    pub entrypoint_kind: EntrypointKind,
    pub rust_file_count: usize,
    pub source_files: Vec<PathBuf>,
    pub module_gaps: Vec<ModuleGap>,
}

impl WorkspaceModel {
    pub fn inspect(goal_text: &str, workspace: &Path) -> Option<Self> {
        let target_root = parse_agent_goal_markdown(goal_text)
            .target_path
            .unwrap_or_else(|| workspace.to_path_buf());
        let path_exists = target_root.exists();
        if !path_exists {
            return Some(Self {
                target_root,
                path_exists,
                repo_initialized: false,
                cargo_toml_exists: false,
                cargo_lock_exists: false,
                crate_name: None,
                src_dir_exists: false,
                entrypoint_kind: EntrypointKind::None,
                rust_file_count: 0,
                source_files: Vec::new(),
                module_gaps: Vec::new(),
            });
        }

        let cargo_toml_exists = target_root.join("Cargo.toml").exists();
        let cargo_lock_exists = target_root.join("Cargo.lock").exists();
        let src_dir = target_root.join("src");
        let src_dir_exists = src_dir.is_dir();
        let main_exists = src_dir.join("main.rs").exists();
        let lib_exists = src_dir.join("lib.rs").exists();
        let source_files = collect_source_files(&target_root);
        let entrypoint_kind = match (main_exists, lib_exists) {
            (true, true) => EntrypointKind::Mixed,
            (true, false) => EntrypointKind::Bin,
            (false, true) => EntrypointKind::Lib,
            (false, false) => EntrypointKind::None,
        };

        Some(Self {
            target_root: target_root.clone(),
            path_exists,
            repo_initialized: target_root.join(".git").exists(),
            cargo_toml_exists,
            cargo_lock_exists,
            crate_name: parse_crate_name(&target_root.join("Cargo.toml")),
            src_dir_exists,
            entrypoint_kind,
            rust_file_count: count_rust_files(&target_root),
            source_files,
            module_gaps: collect_module_gaps(&target_root),
        })
    }

    pub fn planner_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("target_root={}", self.target_root.display()),
            format!("path_exists={}", self.path_exists),
        ];
        if !self.path_exists {
            lines.push("precondition: target workspace missing; first action must create/init it".to_string());
            return lines;
        }

        lines.push(format!("repo_initialized={}", self.repo_initialized));
        lines.push(format!("cargo_project={}", self.cargo_toml_exists));
        if let Some(crate_name) = &self.crate_name {
            lines.push(format!("crate_name={crate_name}"));
        }
        lines.push(format!("src_dir_exists={}", self.src_dir_exists));
        lines.push(format!("entrypoint_kind={}", self.entrypoint_kind.as_str()));
        lines.push(format!("rust_file_count={}", self.rust_file_count));
        if !self.source_files.is_empty() {
            lines.push(format!(
                "file_graph={}",
                self.source_files
                    .iter()
                    .take(8)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.cargo_toml_exists {
            lines.push("precondition: directory exists but is not a Cargo project; prefer cargo init".to_string());
        }
        if self.cargo_toml_exists && self.entrypoint_kind == EntrypointKind::None {
            lines.push("precondition: Cargo project has no src/main.rs or src/lib.rs; create an entrypoint before cargo check".to_string());
        }
        if !self.module_gaps.is_empty() {
            let summary = self
                .module_gaps
                .iter()
                .take(4)
                .map(|gap| {
                    format!(
                        "{} -> {}",
                        gap.module_name,
                        gap.expected_paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!(
                "precondition: missing module files detected; create them before cargo check: {}",
                summary
            ));
        }
        lines
    }
}

impl EntrypointKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bin => "bin",
            Self::Lib => "lib",
            Self::Mixed => "mixed",
        }
    }
}

fn count_rust_files(root: &Path) -> usize {
    let mut total = 0;
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("target") {
            continue;
        }
        if path.is_dir() {
            total += count_rust_files(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            total += 1;
        }
    }
    total
}

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_source_files_inner(root, root, &mut files);
    files.sort();
    files
}

fn collect_source_files_inner(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("target") {
            continue;
        }
        if path.is_dir() {
            collect_source_files_inner(root, &path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_path_buf());
            }
        }
    }
}

fn parse_crate_name(cargo_toml: &Path) -> Option<String> {
    let contents = fs::read_to_string(cargo_toml).ok()?;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let value = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn collect_module_gaps(root: &Path) -> Vec<ModuleGap> {
    let mut gaps = Vec::new();
    for entry in [root.join("src/lib.rs"), root.join("src/main.rs")] {
        if !entry.exists() {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&entry) else {
            continue;
        };
        for module_name in parse_module_declarations(&contents) {
            let parent = entry.parent().unwrap_or(root);
            let expected_rs = parent.join(format!("{module_name}.rs"));
            let expected_mod = parent.join(&module_name).join("mod.rs");
            if !expected_rs.exists() && !expected_mod.exists() {
                gaps.push(ModuleGap {
                    declared_in: entry.clone(),
                    module_name,
                    expected_paths: vec![expected_rs, expected_mod],
                });
            }
        }
    }
    gaps
}

fn parse_module_declarations(contents: &str) -> Vec<String> {
    let mut modules = Vec::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.starts_with("//") || line.contains('{') {
            continue;
        }
        let candidate = line
            .strip_prefix("pub mod ")
            .or_else(|| line.strip_prefix("mod "));
        let Some(candidate) = candidate else {
            continue;
        };
        let name = candidate.trim().trim_end_matches(';').trim();
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            modules.push(name.to_string());
        }
    }
    modules
}
