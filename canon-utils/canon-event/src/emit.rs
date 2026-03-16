use anyhow::Result;
use crate::tlog::{append_event_json, BinarySegmentWriter, CanonEvent};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn tlog_format_is_binary() -> bool {
    match std::env::var("CANON_TLOG_FORMAT") {
        Ok(format) => format.to_lowercase() != "jsonl",
        Err(_) => true,
    }
}

fn binary_dir_from_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        return path.to_path_buf();
    }
    // Already a .tlog.d dir path (not yet created); avoid adding another .d
    if path.to_string_lossy().ends_with(".tlog.d") {
        return path.to_path_buf();
    }
    path.with_extension("tlog.d")
}

pub fn resolve_tlog_path(project_root: Option<&Path>, override_env: Option<&str>) -> PathBuf {
    if let Some(env) = override_env {
        if let Ok(path) = std::env::var(env) {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    if let Some(root) = project_root {
        return root
            .join("state")
            .join("event_log")
            .join("event.tlog.d");
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
}

pub fn emit_event(source: &str, kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
    let canon = CanonEvent::new(source, kind, payload);
    if tlog_format_is_binary() {
        let dir = binary_dir_from_path(tlog_path);
        let writer = BinarySegmentWriter::open(&dir)?;
        let _ = writer.append_event(&canon);
        return Ok(());
    }
    append_event_json(tlog_path, source, kind, canon.payload)?;
    Ok(())
}

pub fn emit_rustc_event(kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
    emit_event("canon-rustc", kind, payload, tlog_path)
}

pub fn emit_runtime_event(kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
    emit_event("canon-runtime", kind, payload, tlog_path)
}

pub fn emit_edit_event(payload: Value, project_root: &Path) -> Result<()> {
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    emit_event("canon-editor", "edit_event", payload, &tlog_path)
}

pub fn emit_capability_event(
    source: &str,
    kind: &str,
    payload: Value,
    tlog_path: &Path,
) -> Result<()> {
    emit_event(source, kind, payload, tlog_path)
}
