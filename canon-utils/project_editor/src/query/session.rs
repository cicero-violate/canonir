use anyhow::{anyhow, Result};
use crate::symbol_index::SymbolIndex;
use canon_query::{TlogReader, TlogRecord};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct AnalysisSession {
    pub module_files: HashMap<String, PathBuf>,
    pub file_modules: HashMap<PathBuf, Vec<String>>,
    pub files: HashSet<PathBuf>,
    pub uses_crate_prefix: bool,
    tlog_path: PathBuf,
}

impl AnalysisSession {
    pub fn load(project_root: &Path) -> Result<Self> {
        let index = SymbolIndex::build(project_root)?;
        if index.module_files().is_empty() {
            return Err(anyhow!(
                "tlog has no module mapping for {}",
                project_root.display()
            ));
        }
        let tlog_path = project_root.join("state/kernel_logs/kernel.tlog");
        Ok(Self {
            module_files: index.module_files().clone(),
            file_modules: index.file_modules().clone(),
            files: index.files().clone(),
            uses_crate_prefix: index.uses_crate_prefix(),
            tlog_path,
        })
    }

    pub fn edges_by_kind(&self, edge_kind: &str) -> Result<Vec<TlogRecord>> {
        let idx_path = self.tlog_path.with_extension("tlog.idx");
        let offset = if idx_path.exists() {
            TlogReader::last_session_offset(&idx_path).unwrap_or(0)
        } else {
            0
        };
        TlogReader::query_by_kind(&self.tlog_path, edge_kind, offset)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn callers_of(&self, sym: &str) -> Result<Vec<TlogRecord>> {
        let idx_path = self.tlog_path.with_extension("tlog.idx");
        let offset = if idx_path.exists() {
            TlogReader::last_session_offset(&idx_path).unwrap_or(0)
        } else {
            0
        };
        TlogReader::query_callers(&self.tlog_path, sym, offset)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}
