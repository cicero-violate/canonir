use anyhow::Result;
use canon_event_log::info;
use std::path::{Path, PathBuf};

pub fn emit_analysis_event(
    _tlog_path: &Path,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    info("canon-analysis", kind, payload);
    Ok(())
}

pub fn resolve_tlog_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_REPORTS_TLOG") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d")
}
