use anyhow::Result;
use std::path::Path;

pub fn emit_analysis_event(tlog_path: &Path, kind: &str, payload: serde_json::Value) -> Result<()> {
    canon_meta::canon_emit_meta!("canon-analysis", kind, payload.clone(), tlog_path)?;
    Ok(())
}
