use anyhow::Result;
use canon_event_emit::emit_edit_event;
use canon_types::EditEvent;
use std::path::Path;

pub fn publish_edit_event(project_root: &Path, event: EditEvent) -> Result<()> {
    let payload = serde_json::to_value(&event)?;
    emit_edit_event(payload, project_root)?;
    Ok(())
}
