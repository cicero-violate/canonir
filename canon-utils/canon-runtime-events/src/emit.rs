use anyhow::Result;
use crate::tlog::{emit_event_json, BinarySegmentWriter, TlogEvent};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

thread_local! {
    static WRITER_CACHE: RefCell<HashMap<PathBuf, BinarySegmentWriter>> = RefCell::new(HashMap::new());
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

/// Write a `TlogEvent` to the given path, selecting the format automatically:
/// - directory or `.tlog.d` path → `BinarySegmentWriter` (JSONL segments)
/// - file path + `CANON_TLOG_FORMAT=jsonl` → `emit_event_json` (single JSONL file)
/// - file path (default) → `BinarySegmentWriter` in adjacent `.tlog.d` directory
pub fn write_event_auto(path: &Path, event: &TlogEvent) -> Result<()> {
    let path_str = path.to_string_lossy();
    let is_segment_dir = path.is_dir() || path_str.ends_with(".tlog.d");
    let force_jsonl = matches!(
        std::env::var("CANON_TLOG_FORMAT").as_deref(),
        Ok("jsonl") | Ok("JSONL")
    );

    if is_segment_dir || !force_jsonl {
        let dir = if is_segment_dir {
            path.to_path_buf()
        } else {
            path.with_extension("tlog.d")
        };
        WRITER_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if !cache.contains_key(&dir) {
                cache.insert(dir.clone(), BinarySegmentWriter::open(dir.as_path())?);
            }
            cache.get_mut(&dir).unwrap().write_event(event)
        })
    } else {
        emit_event_json(path, &event.source, &event.kind, event.payload.clone())
    }
}

pub fn emit_event(source: &str, kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
    let event = TlogEvent::new(source, kind, payload);
    write_event_auto(tlog_path, &event)
}

pub fn emit_edit_event(payload: Value, project_root: &Path) -> Result<()> {
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    emit_event("canon-editor", "edit_event", payload, tlog_path.as_path())
}
