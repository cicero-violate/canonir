use crate::tlog::{emit_canon_event_json, BinarySegmentWriter};
use crate::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
use anyhow::Result;
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
        return root.join("state").join("event_log").join("event.tlog.d");
    }
    PathBuf::from("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
}

pub(crate) fn emit_event(source: &str, kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
    let payload_json = serde_json::to_value(payload).expect("emit_event payload serialization failed");
    let payload = CanonPayload {
        input: serde_json::json!({}),
        output: serde_json::json!({}),
        delta: serde_json::json!({}),
        meta: CanonPayloadMeta { file: file!().to_string(), line: line!() },
        data: payload_json,
    };
    let kind_enum = match kind {
        "edit_event" | "edit" => EventKind::Edit,
        "runtime_started" => EventKind::RuntimeStarted,
        _ => EventKind::Debug,
    };
    let event = CanonEvent::new(EventId::new(new_event_id()), Vec::new(), source.to_string(), kind_enum, now_millis(), payload, true);
    write_canon_event_auto(tlog_path, &event)
}

pub fn emit_edit_event(payload: Value, project_root: &Path) -> Result<()> {
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    emit_event("canon-editor", "edit_event", payload, tlog_path.as_path())
}

/// Write a `CanonEvent` to the given path, selecting the format automatically.
pub fn write_canon_event_auto(path: &Path, event: &CanonEvent) -> Result<()> {
    let path_str = path.to_string_lossy();
    let is_segment_dir = path.is_dir() || path_str.ends_with(".tlog.d");
    let force_jsonl = matches!(std::env::var("CANON_TLOG_FORMAT").as_deref(), Ok("jsonl") | Ok("JSONL"));

    if is_segment_dir || !force_jsonl {
        let dir = if is_segment_dir { path.to_path_buf() } else { path.with_extension("tlog.d") };
        WRITER_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if !cache.contains_key(&dir) {
                cache.insert(dir.clone(), BinarySegmentWriter::open(dir.as_path())?);
            }
            cache.get_mut(&dir).unwrap().write_canon_event(event)
        })
    } else {
        emit_canon_event_json(path, event)
    }
}

pub fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
