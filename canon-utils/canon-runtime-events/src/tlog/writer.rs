use anyhow::Result;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::CanonEvent;

#[allow(dead_code)]
pub(crate) fn emit_canon_event_json(path: &Path, event: &CanonEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(event)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}
