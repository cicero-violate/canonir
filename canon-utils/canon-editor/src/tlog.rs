use anyhow::Result;
use canon_event_log::info;
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use canon_types::EditEvent;
use std::path::{Path, PathBuf};

fn default_tlog_path(project_root: &Path) -> PathBuf {
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return PathBuf::from(path);
    }
    project_root
        .join("state")
        .join("kernel_logs")
        .join("kernel.tlog.d")
}

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
    path.with_extension("tlog.d")
}

pub fn publish_edit_event(project_root: &Path, event: EditEvent) -> Result<()> {
    let payload = serde_json::to_value(&event)?;
    if tlog_format_is_binary() {
        let path = default_tlog_path(project_root);
        let dir = binary_dir_from_path(&path);
        let writer = BinarySegmentWriter::open(&dir)?;
        let canon = CanonEvent::new("canon-editor", "edit_event", payload.clone());
        let _ = writer.append_event(&canon);
    } else {
        let path = default_tlog_path(project_root);
        append_event_json(&path, "canon-editor", "edit_event", payload.clone())?;
    }
    info("canon-editor", "edit_event", payload);
    Ok(())
}
