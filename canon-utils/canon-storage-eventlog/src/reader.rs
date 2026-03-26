use canon_event::{CanonEvent, CanonPayload, CanonPayloadMeta, EditEvent, RustcEvent};
use std::fs;
use std::io::{BufRead, Read};
use std::path::Path;

use crate::binary_reader::{is_binary_magic, read_binary_events};

#[derive(Debug, Clone)]
pub enum AnyEvent {
    Canon(CanonEvent),
    Code(RustcEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlogFormat {
    Jsonl,
    Binary,
}

pub fn parse_any_event(line: &str) -> Option<AnyEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(event) = serde_json::from_str::<CanonEvent>(trimmed) {
        return Some(AnyEvent::Canon(event));
    }
    if let Ok(legacy) = serde_json::from_str::<LegacyCanonEvent>(trimmed) {
        return Some(AnyEvent::Canon(upgrade_legacy_event(legacy)));
    }
    None
}

pub fn parse_rustc_event_value(value: &serde_json::Value) -> Option<RustcEvent> {
    serde_json::from_value(value.clone()).ok()
}

pub fn parse_edit_event_value(value: &serde_json::Value) -> Option<EditEvent> {
    serde_json::from_value(value.clone()).ok()
}

pub fn extract_rustc_event(canon: &CanonEvent) -> Option<RustcEvent> {
    if canon.kind == canon_event::EventKind::Code {
        return parse_rustc_event_value(&canon.payload.data);
    }
    None
}

pub fn extract_edit_event(canon: &CanonEvent) -> Option<EditEvent> {
    if canon.kind == canon_event::EventKind::Edit {
        return parse_edit_event_value(&canon.payload.data);
    }
    None
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SupervisorEvent {
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

pub fn extract_supervisor_event(canon: &CanonEvent) -> Option<SupervisorEvent> {
    if canon.kind == canon_event::EventKind::SupervisorEvent {
        return serde_json::from_value(canon.payload.data.clone()).ok();
    }
    None
}

pub fn detect_tlog_format(path: &Path) -> TlogFormat {
    if path.is_dir() {
        return TlogFormat::Binary;
    }
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return TlogFormat::Jsonl,
    };
    let mut head = [0u8; 4];
    if file.read_exact(&mut head).is_ok() {
        if is_binary_magic(&head) {
            return TlogFormat::Binary;
        }
    }
    TlogFormat::Jsonl
}

// Legacy schema support ------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct LegacyCanonEvent {
    pub event_id: Option<u64>,
    pub meta: LegacyEventMeta,
    pub kind: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
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

pub fn read_any_events(path: &Path) -> anyhow::Result<Vec<AnyEvent>> {
    match detect_tlog_format(path) {
        TlogFormat::Binary => {
            let events = read_binary_events(path)?;
            Ok(events.into_iter().map(AnyEvent::Canon).collect())
        }
        TlogFormat::Jsonl => {
            let file = fs::File::open(path)?;
            let reader = std::io::BufReader::new(file);
            let mut out = Vec::new();
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                        // Fallback: file may actually be binary.
                        if let Ok(events) = read_binary_events(path) {
                            return Ok(events.into_iter().map(AnyEvent::Canon).collect());
                        }
                        return Ok(out);
                    }
                    Err(err) => return Err(err.into()),
                };
                if let Some(event) = parse_any_event(&line) {
                    out.push(event);
                }
            }
            Ok(out)
        }
    }
}

pub fn read_any_events_from_path(path: &Path) -> anyhow::Result<Vec<AnyEvent>> {
    if path.is_dir() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("log") {
                continue;
            }
            if !p.is_file() {
                continue;
            }
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                if let Ok(seq) = stem.parse::<u64>() {
                    entries.push((seq, p));
                }
            }
        }
        entries.sort_by_key(|(seq, _)| *seq);
        let mut out = Vec::new();
        for (_, p) in entries {
            let events = read_binary_events(&p)?;
            out.extend(events.into_iter().map(AnyEvent::Canon));
        }
        Ok(out)
    } else {
        read_any_events(path)
    }
}

pub fn read_any_events_from_path_with_start_seq(path: &Path, start_seq: u64) -> anyhow::Result<Vec<AnyEvent>> {
    if path.is_dir() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("log") {
                continue;
            }
            if !p.is_file() {
                continue;
            }
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                if let Ok(seq) = stem.parse::<u64>() {
                    entries.push((seq, p));
                }
            }
        }
        entries.sort_by_key(|(seq, _)| *seq);
        let mut out = Vec::new();
        for (seq, p) in entries {
            if seq < start_seq {
                // Segment is entirely before our window — skip it.
                continue;
            }
            let events = read_binary_events(&p)?;
            out.extend(events.into_iter().map(AnyEvent::Canon));
        }
        Ok(out)
    } else {
        read_any_events(path)
    }
}
