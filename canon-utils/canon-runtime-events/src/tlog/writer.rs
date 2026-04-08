use anyhow::Result;
use std::fs::{self};
use std::path::Path;

use crate::CanonEvent;
use crate::tlog::binary::BinarySegmentWriter;

#[allow(dead_code)]
pub(crate) fn emit_canon_event_json(path: &Path, event: &CanonEvent) -> Result<()> {
    // Redirect to canonical binary segment writer instead of JSONL
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("invalid tlog path: no parent directory"))?;
    fs::create_dir_all(dir)?;

    let writer = BinarySegmentWriter::open(dir)?;
    writer.write_canon_event(event)?;

    Ok(())
}
