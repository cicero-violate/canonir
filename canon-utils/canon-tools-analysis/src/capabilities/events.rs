use anyhow::Result;
use canon_event::emit_debug::info;
use canon_event::emit_event;
use std::path::Path;

pub fn emit_analysis_event(
    tlog_path: &Path,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    emit_event("canon-analysis", kind, payload.clone(), tlog_path)?;
    info("canon-analysis", kind, payload);
    Ok(())
}
