use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::Path;

use crate::panic_capture::PanicRecord;

#[derive(Serialize)]
struct PanicSummary {
    panic_count: usize,
    sample_messages: Vec<String>,
}

pub fn build_panic_report(log_path: &Path, summary_path: &Path) -> Result<()> {
    if !log_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(log_path)?;
    let mut records: Vec<PanicRecord> = Vec::new();
    for line in content.lines() {
        if let Ok(rec) = serde_json::from_str::<PanicRecord>(line) {
            records.push(rec);
        }
    }
    if records.is_empty() {
        return Ok(());
    }
    let mut sample = Vec::new();
    for rec in records.iter().take(5) {
        sample.push(rec.message.clone());
    }
    let summary = PanicSummary {
        panic_count: records.len(),
        sample_messages: sample,
    };
    fs::write(summary_path, serde_json::to_string_pretty(&summary)?)?;
    Ok(())
}
