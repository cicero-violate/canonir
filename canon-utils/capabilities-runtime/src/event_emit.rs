use anyhow::Result;
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use serde_json::json;
use std::path::PathBuf;

fn default_tlog_path() -> PathBuf {
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    let binary = PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d");
    if binary.exists() {
        return binary;
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog")
}

fn binary_dir_from_path(path: &PathBuf) -> PathBuf {
    if path.is_dir() {
        return path.clone();
    }
    path.with_extension("tlog.d")
}

fn tlog_format_is_binary() -> bool {
    match std::env::var("CANON_TLOG_FORMAT") {
        Ok(format) => format.to_lowercase() != "jsonl",
        Err(_) => true,
    }
}

fn emit_event(kind: &str, payload: serde_json::Value) -> Result<()> {
    let event = CanonEvent::new("canon-capability-runtime", kind, payload);
    if tlog_format_is_binary() {
        let path = default_tlog_path();
        let dir = binary_dir_from_path(&path);
        let writer = BinarySegmentWriter::open(&dir)?;
        let _ = writer.append_event(&event);
        return Ok(());
    }
    let path = default_tlog_path();
    append_event_json(&path, "canon-capability-runtime", kind, event.payload)?;
    Ok(())
}

pub fn emit_workspace_changed(crate_name: &str) -> Result<()> {
    emit_event("workspace.changed", json!({ "crate": crate_name }))
}

pub fn emit_build_started(crate_name: &str) -> Result<()> {
    emit_event("build.started", json!({ "crate": crate_name }))
}

pub fn emit_build_completed(crate_name: &str, success: bool, duration_ms: u128) -> Result<()> {
    emit_event(
        "build.completed",
        json!({ "crate": crate_name, "success": success, "duration_ms": duration_ms }),
    )
}

pub fn emit_run_started(crate_name: &str, bin: Option<&str>) -> Result<()> {
    emit_event("run.started", json!({ "crate": crate_name, "bin": bin }))
}

pub fn emit_run_completed(
    crate_name: &str,
    bin: Option<&str>,
    success: bool,
    duration_ms: u128,
) -> Result<()> {
    emit_event(
        "run.completed",
        json!({
            "crate": crate_name,
            "bin": bin,
            "success": success,
            "duration_ms": duration_ms
        }),
    )
}

pub fn emit_check_started(crate_name: &str) -> Result<()> {
    emit_event("check.started", json!({ "crate": crate_name }))
}

pub fn emit_check_completed(crate_name: &str, success: bool, duration_ms: u128) -> Result<()> {
    emit_event(
        "check.completed",
        json!({ "crate": crate_name, "success": success, "duration_ms": duration_ms }),
    )
}
