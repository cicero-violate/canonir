use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ReportLayout {
    base: PathBuf,
    crate_name: Option<String>,
    /// When true, crate_root() returns base directly (no crates/{name}/ wrapping).
    direct: bool,
}

impl ReportLayout {
    pub fn from_crate_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut base = root.clone();
        let mut crate_name = None;
        if let Some(name) = root.file_name().and_then(|s| s.to_str()) {
            if let Some(parent) = root.parent() {
                if parent.file_name().and_then(|s| s.to_str()) == Some("crates") {
                    crate_name = Some(name.to_string());
                    if let Some(grand) = parent.parent() {
                        base = grand.to_path_buf();
                    } else {
                        base = parent.to_path_buf();
                    }
                }
            }
        }
        Self { base, crate_name, direct: false }
    }

    /// Layout whose crate_root() is exactly `root` — no crates/{name}/ wrapping.
    /// Used for workspace-level reports written to a flat directory.
    pub fn from_direct_root(root: impl Into<PathBuf>) -> Self {
        Self { base: root.into(), crate_name: None, direct: true }
    }

    pub fn from_workspace_root(root: impl Into<PathBuf>, crate_name: impl Into<String>) -> Self {
        Self { base: root.into(), crate_name: Some(crate_name.into()), direct: false }
    }

    pub fn crate_root(&self) -> PathBuf {
        if self.direct {
            return self.base.clone();
        }
        let name = self.crate_name.as_deref().unwrap_or("unknown");
        self.base.join("crates").join(name)
    }

    pub fn workspace_root(&self) -> PathBuf {
        self.base.join("workspace")
    }

    pub fn graph_dir(&self) -> PathBuf {
        self.crate_root().join("graph")
    }

    pub fn graphs_dir(&self) -> PathBuf {
        self.crate_root().join("graphs")
    }

    pub fn analysis_dir(&self) -> PathBuf {
        self.crate_root().join("analysis")
    }

    pub fn metrics_dir(&self) -> PathBuf {
        self.crate_root().join("metrics")
    }

    pub fn invariants_dir(&self) -> PathBuf {
        self.crate_root().join("invariants")
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.crate_root().join("meta")
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [self.graph_dir(), self.graphs_dir(), self.analysis_dir(), self.metrics_dir(), self.invariants_dir(), self.meta_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.base
    }
}
