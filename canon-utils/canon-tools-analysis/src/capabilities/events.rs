use anyhow::Result;
use canon_event::canon_emit;
use std::path::Path;

pub fn emit_analysis_event(tlog_path: &Path, kind: &str, payload: serde_json::Value) -> Result<()> {
    canon_emit!("canon-analysis", kind, payload.clone(), tlog_path)?;
    Ok(())
}
