use anyhow::Result;
use canon_event::{new_error_occurred, resolve_tlog_path, write_shaped_event_auto, CanonPayloadMeta, EditEvent, EventKind, InvariantDiscovered};
use std::path::Path;

pub fn publish_edit_event(project_root: &Path, event: EditEvent) -> Result<()> {
    let payload = serde_json::to_value(&event)?;
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    canon_meta::canon_emit_meta!("canon-editor", "edit_event", payload, &tlog_path)
}

pub fn publish_invariant_error(project_root: &Path, feature: &str, message: &str, context: serde_json::Value) {
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    let meta = CanonPayloadMeta { file: file!().to_string(), line: line!() };
    let parent = write_shaped_event_auto(
        &tlog_path,
        "canon-editor",
        EventKind::InvariantDiscovered,
        &InvariantDiscovered { feature: feature.to_string(), confidence: 1.0, support: 1 },
        Vec::new(),
        true,
        meta.clone(),
    )
    .ok();
    let _ = write_shaped_event_auto(
        &tlog_path,
        "canon-editor",
        EventKind::ErrorOccurred,
        &new_error_occurred("editor_invariant_violation", "canon-tools-editor", message.to_string(), "error", context, None),
        parent.into_iter().collect(),
        true,
        meta,
    );
}
