use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::BufRead;
use std::path::Path;

use crate::replay::session_scan::find_last_session_offset;
use crate::replay::tlog_reader::parse_tlog_event;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TlogIntegrityReport {
    pub tlog_path: String,
    pub file_size: u64,
    pub line_count: u64,
    pub session_count: u64,
    pub last_session_offset_idx: Option<u64>,
    pub last_session_offset_found: bool,
    pub session_offsets_monotonic: bool,
    pub parse_errors: u64,
    pub hash_chain_last: u64,
    pub replay_determinism_ok: bool,
}

pub fn write_tlog_integrity_report(tlog_path: &Path, reports_dir: &Path) -> Result<()> {
    fs::create_dir_all(reports_dir)?;
    let file = fs::File::open(tlog_path)?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let idx_offset = find_last_session_offset(tlog_path);
    let reader = std::io::BufReader::new(&file);
    let mut offset: u64 = 0;
    let mut line_count = 0u64;
    let mut session_count = 0u64;
    let mut parse_errors = 0u64;
    let mut last_session_offset_found = false;
    let mut session_offsets_monotonic = true;
    let mut last_session_offset_seen: Option<u64> = None;
    let mut hash_chain: u64 = 0;

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let line_start = offset;
        offset = offset.saturating_add(raw_line.as_bytes().len() as u64 + 1);
        line_count += 1;
        let mut line = raw_line.as_str();
        let mut slice_offset = line_start;
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(value) = parse_tlog_event(prefix) {
                        if apply_tlog_integrity_record(&value, slice_offset, &idx_offset, &mut session_count, &mut last_session_offset_found, &mut session_offsets_monotonic, &mut last_session_offset_seen) {
                            // ok
                        }
                    } else {
                        parse_errors += 1;
                    }
                    slice_offset = slice_offset.saturating_add(idx as u64);
                    line = suffix;
                    continue;
                }
            }
            if let Some(value) = parse_tlog_event(line) {
                if apply_tlog_integrity_record(&value, slice_offset, &idx_offset, &mut session_count, &mut last_session_offset_found, &mut session_offsets_monotonic, &mut last_session_offset_seen) {
                    // ok
                }
            } else if !line.trim().is_empty() {
                parse_errors += 1;
            }
            break;
        }
        hash_chain = hash_chain.wrapping_mul(1315423911).wrapping_add(hash_bytes(raw_line.as_bytes()));
    }

    let report = TlogIntegrityReport {
        tlog_path: tlog_path.to_string_lossy().to_string(),
        file_size,
        line_count,
        session_count,
        last_session_offset_idx: idx_offset,
        last_session_offset_found,
        session_offsets_monotonic,
        parse_errors,
        hash_chain_last: hash_chain,
        replay_determinism_ok: parse_errors == 0 && session_offsets_monotonic,
    };
    fs::write(reports_dir.join("tlog_integrity.json"), serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

fn apply_tlog_integrity_record(
    value: &Value,
    line_start: u64,
    idx_offset: &Option<u64>,
    session_count: &mut u64,
    last_session_offset_found: &mut bool,
    session_offsets_monotonic: &mut bool,
    last_session_offset_seen: &mut Option<u64>,
) -> bool {
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return false;
    };
    if tag != "SESSION" {
        return true;
    }
    *session_count += 1;
    let byte_offset = value.get("byte_offset").and_then(|v| v.as_u64());
    if let Some(offset) = byte_offset {
        if Some(offset) == *idx_offset {
            *last_session_offset_found = true;
        }
        if let Some(prev) = *last_session_offset_seen {
            if offset < prev {
                *session_offsets_monotonic = false;
            }
        }
        *last_session_offset_seen = Some(offset);
        if offset != line_start {
            *session_offsets_monotonic = false;
        }
    }
    true
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}
