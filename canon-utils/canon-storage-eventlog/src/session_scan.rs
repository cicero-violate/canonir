use std::fs;
use std::io::BufRead;
use std::path::Path;

use canon_event::RustcEvent;

use crate::reader::{extract_rustc_event, parse_any_event, AnyEvent};

pub fn find_last_session_offset(tlog_path: &Path) -> Option<u64> {
    if tlog_path.is_dir() {
        return None;
    }
    let idx_path = tlog_path.with_extension("tlog.idx");
    let data = fs::read_to_string(idx_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&data).ok()?;
    value.get("last_session_offset").and_then(|v| v.as_u64())
}

pub fn find_last_graph_session_offset(tlog_path: &Path) -> Option<u64> {
    if tlog_path.is_dir() {
        return None;
    }
    let file = fs::File::open(tlog_path).ok()?;
    let reader = std::io::BufReader::new(file);
    let mut offset: u64 = 0;
    let mut current_session_offset: Option<u64> = None;
    let mut current_has_graph = false;
    let mut last_with_graph: Option<u64> = None;

    for raw_line in reader.lines() {
        let raw_line = raw_line.ok()?;
        let line_start = offset;
        offset = offset.saturating_add(raw_line.as_bytes().len() as u64 + 1);
        if let Some(event) = parse_any_event(&raw_line) {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_rustc_event(&canon) {
                    match kernel {
                        RustcEvent::SessionStart(_) => {
                            if current_has_graph {
                                if let Some(off) = current_session_offset {
                                    last_with_graph = Some(off);
                                }
                            }
                            current_session_offset = Some(line_start);
                            current_has_graph = false;
                        }
                        RustcEvent::NodeDefined(_) | RustcEvent::EdgeDefined(_) => {
                            current_has_graph = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if current_has_graph {
        if let Some(off) = current_session_offset {
            last_with_graph = Some(off);
        }
    }
    last_with_graph
}

pub fn session_contains_module_nodes(tlog_path: &Path) -> bool {
    if tlog_path.is_dir() {
        // Binary segment directory: if any .log files exist, assume module nodes are present.
        return std::fs::read_dir(tlog_path).ok().map(|mut d| d.any(|e| e.ok().and_then(|e| e.path().extension().map(|x| x == "log")).unwrap_or(false))).unwrap_or(false);
    }
    let Ok(mut file) = fs::File::open(tlog_path) else {
        return false;
    };
    if let Some(offset) = find_last_session_offset(tlog_path) {
        use std::io::Seek;
        use std::io::SeekFrom;
        let _ = file.seek(SeekFrom::Start(offset));
    }
    let reader = std::io::BufReader::new(file);
    for raw_line in reader.lines().flatten() {
        if let Some(event) = parse_any_event(&raw_line) {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_rustc_event(&canon) {
                    if matches!(kernel, RustcEvent::NodeDefined(canon_event::NodeDefined { kind, .. }) if kind == "MODULE") {
                        return true;
                    }
                }
            }
        }
    }
    false
}
