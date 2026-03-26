use anyhow::Result;
use canon_event::{CanonEvent, CanonPayload, CanonPayloadMeta};
use serde::Deserialize;
use std::fs;
use std::path::Path;
//
const MAGIC: u32 = 0x544C4F47; // "TLOG"

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u64(buf: &[u8]) -> u64 {
    u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]])
}

pub fn read_binary_events(path: &Path) -> Result<Vec<CanonEvent>> {
    let bytes = fs::read(path)?;
    // Legacy binary segment — skip silently.
    if is_binary_magic(&bytes) {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    let content = std::str::from_utf8(&bytes).unwrap_or("");
    for raw_line in content.split('\n') {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<CanonEvent>(trimmed) {
            events.push(e);
            continue;
        }
        if let Ok(legacy) = serde_json::from_str::<LegacyCanonEvent>(trimmed) {
            events.push(upgrade_legacy_event(legacy));
        }
    }
    Ok(events)
}

pub fn is_binary_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && read_u32(&bytes[0..4]) == MAGIC
}

pub fn read_binary_events_from_segment_with_start_seq(log_path: &Path, start_seq: u64) -> Result<Vec<CanonEvent>> {
    let idx_path = log_path.with_extension("idx");
    let mut start_pos = 0u64;
    if idx_path.exists() {
        let idx_bytes = fs::read(&idx_path)?;
        let mut cursor = 0usize;
        while cursor + 16 <= idx_bytes.len() {
            let seq = read_u64(&idx_bytes[cursor..cursor + 8]);
            let pos = read_u64(&idx_bytes[cursor + 8..cursor + 16]);
            if seq <= start_seq {
                start_pos = pos;
            } else {
                break;
            }
            cursor += 16;
        }
    }

    let bytes = fs::read(log_path)?;
    if is_binary_magic(&bytes) {
        return Ok(Vec::new());
    }

    let relevant = bytes.get(start_pos as usize..).unwrap_or(&bytes);
    let content = std::str::from_utf8(relevant).unwrap_or("");
    let mut events = Vec::new();
    for raw_line in content.split('\n') {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<CanonEvent>(trimmed) {
            events.push(e);
            continue;
        }
        if let Ok(legacy) = serde_json::from_str::<LegacyCanonEvent>(trimmed) {
            events.push(upgrade_legacy_event(legacy));
        }
    }
    Ok(events)
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyCanonEvent {
    pub event_id: Option<u64>,
    pub meta: LegacyEventMeta,
    pub kind: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyEventMeta {
    pub ts: u64,
    pub source: String,
    pub file: String,
    pub line: u32,
}

fn upgrade_legacy_event(old: LegacyCanonEvent) -> CanonEvent {
    CanonEvent {
        id: canon_event::EventId::new(old.event_id.map(|v| v.to_string()).unwrap_or_else(|| canon_event::new_event_id())),
        parent_ids: Vec::new(),
        actor: old.meta.source,
        kind: serde_json::from_str::<canon_event::EventKind>(&format!("\"{}\"", old.kind)).unwrap_or(canon_event::EventKind::Debug),
        ts: old.meta.ts,
        payload: CanonPayload {
            input: serde_json::json!({}),
            output: serde_json::json!({}),
            delta: serde_json::json!({}),
            meta: CanonPayloadMeta { file: old.meta.file, line: old.meta.line },
            data: old.data.unwrap_or_else(|| serde_json::json!({})),
        },
        prev_event_id: None,
    }
}
