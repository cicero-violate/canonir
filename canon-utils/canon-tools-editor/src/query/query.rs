use super::session::{AnalysisSession, GraphEdgeRecord};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn query_map<K, V>(map: &HashMap<K, V>, key: &K) -> Option<V>
where
    K: std::hash::Hash + Eq,
    V: Clone,
{
    map.get(key).cloned()
}

pub fn module_file(session: &AnalysisSession, module_path: &str) -> Result<PathBuf> {
    query_map(&session.module_files, &module_path.to_string()).ok_or_else(|| anyhow!("analysis missing module path {module_path}"))
}

pub fn file_modules(session: &AnalysisSession, file: &Path) -> Vec<String> {
    query_map(&session.file_modules, &file.to_path_buf()).unwrap_or_default()
}

pub fn callers_of(session: &AnalysisSession, sym: &str) -> Result<Vec<GraphEdgeRecord>> {
    session.callers_of(sym)
}

pub fn edges_by_kind(session: &AnalysisSession, kind: &str) -> Result<Vec<GraphEdgeRecord>> {
    session.edges_by_kind(kind)
}
