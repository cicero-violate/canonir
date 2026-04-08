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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapCommandChoice {
    CargoNew,
    CargoInit,
    NoBootstrapNeeded,
}

impl WorkspaceModel {
    pub fn inspect(goal_text: &str, workspace: &Path) -> Option<Self> {
        // Prefer goal-derived path, but fall back to actual workspace if it doesn't exist
        let parsed = parse_agent_goal_markdown(goal_text).target_path
            .or_else(|| extract_path_from_goal_text(goal_text));
        // Extract explicit path from goal text (backticks) to preserve missing-path semantics
        let explicit_goal_path = goal_text
            .split('`')
            .nth(1)
            .map(PathBuf::from);
        // Prefer explicit goal path if present, even if it does not exist
        let (target_root, path_exists) = if let Some(raw) = explicit_goal_path {
            let exists = raw.exists();
            (raw, exists)
        } else if let Some(p) = parsed {
            let exists = p.exists();
            (p, exists)
        } else {
            // 🔧 FIX: do NOT fall back to workspace for semantic modeling
            // Missing explicit/parsed target must be treated as non-existent
            let fallback = workspace.to_path_buf();
            (fallback, false)
        };
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
        let mut lines = vec![format!("target_root={}", self.target_root.display()), format!("path_exists={}", self.path_exists)];
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
            lines.push(format!("file_graph={}", self.source_files.iter().take(8).map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")));
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
                .map(|gap| format!("{} -> {}", gap.module_name, gap.expected_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" or ")))
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("precondition: missing module files detected; create them before cargo check: {}", summary));
        }
        lines
    }
}

impl BootstrapCommandChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CargoNew => "cargo_new",
            Self::CargoInit => "cargo_init",
            Self::NoBootstrapNeeded => "no_bootstrap_needed",
        }
    }
}

pub fn select_bootstrap_command(target_root: &Path) -> BootstrapCommandChoice {
    if !target_root.exists() {
        return BootstrapCommandChoice::CargoNew;
    }
    if !target_root.join("Cargo.toml").exists() {
        return BootstrapCommandChoice::CargoInit;
    }
    BootstrapCommandChoice::NoBootstrapNeeded
}

pub fn semantic_state_matches_workspace_model(path_exists: bool, cargo_project: bool, model: &WorkspaceModel) -> bool {
    path_exists == model.path_exists && cargo_project == model.cargo_toml_exists
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

#[cfg(test)]
mod tests {
    use super::{select_bootstrap_command, semantic_state_matches_workspace_model, BootstrapCommandChoice, WorkspaceModel};

    #[test]
    fn select_bootstrap_command_uses_cargo_new_for_missing_dir() {
        let root = std::env::temp_dir().join(format!("canon_bootstrap_missing_{}", uuid::Uuid::new_v4()));
        assert_eq!(select_bootstrap_command(&root), BootstrapCommandChoice::CargoNew);
    }

    #[test]
    fn select_bootstrap_command_uses_cargo_init_for_existing_non_cargo_dir() {
        let root = std::env::temp_dir().join(format!("canon_bootstrap_init_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(select_bootstrap_command(&root), BootstrapCommandChoice::CargoInit);
    }

    #[test]
    fn select_bootstrap_command_rejects_bootstrap_for_existing_cargo_project() {
        let root = std::env::temp_dir().join(format!("canon_bootstrap_existing_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"bootstrap_existing\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        assert_eq!(select_bootstrap_command(&root), BootstrapCommandChoice::NoBootstrapNeeded);
    }

    #[test]
    fn semantic_state_matches_workspace_model_detects_state_vs_reality_mismatch() {
        let root = std::env::temp_dir().join(format!("canon_state_match_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"state_match\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let model = WorkspaceModel::inspect(&format!("# Goal\n\n## Target\n- Project path: `{}`\n", root.display()), &root).unwrap();
        assert!(!semantic_state_matches_workspace_model(false, false, &model));
        assert!(semantic_state_matches_workspace_model(true, true, &model));
    }
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

fn extract_path_from_goal_text(goal_text: &str) -> Option<PathBuf> {
    for line in goal_text.lines() {
        if let Some(start) = line.find('`') {
            if let Some(end) = line[start + 1..].find('`') {
                let path = &line[start + 1..start + 1 + end];
                if !path.trim().is_empty() {
                    return Some(PathBuf::from(path.trim()));
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
                gaps.push(ModuleGap { declared_in: entry.clone(), module_name, expected_paths: vec![expected_rs, expected_mod] });
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
        let candidate = line.strip_prefix("pub mod ").or_else(|| line.strip_prefix("mod "));
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
