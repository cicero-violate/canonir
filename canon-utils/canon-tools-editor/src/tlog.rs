use anyhow::Result;
use canon_event::{resolve_tlog_path, EditEvent};
use std::path::Path;

pub fn publish_edit_event(project_root: &Path, event: EditEvent) -> Result<()> {
    let payload = serde_json::to_value(&event)?;
    let tlog_path = resolve_tlog_path(Some(project_root), None);
    canon_meta::canon_emit_meta!("canon-editor", "edit_event", payload, &tlog_path)
}
