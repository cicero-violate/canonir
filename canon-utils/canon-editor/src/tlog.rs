use anyhow::Result;
use canon_event_log::info;
use canon_types::EditEvent;
use std::path::Path;

pub fn publish_edit_event(project_root: &Path, event: EditEvent) -> Result<()> {
    let payload = serde_json::to_value(&event)?;
    let _ = project_root;
    info("canon-editor", "edit_event", payload);
    Ok(())
}
