use anyhow::Result;
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use std::path::{Path, PathBuf};

pub fn emit_analysis_event(
    tlog_path: &Path,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let canon = CanonEvent::new("canon-analysis", kind, payload);
    if tlog_path.is_dir() {
        let writer = BinarySegmentWriter::open(tlog_path)?;
        let _ = writer.append_event(&canon);
        return Ok(());
    }
    append_event_json(tlog_path, "canon-analysis", kind, canon.payload)?;
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
