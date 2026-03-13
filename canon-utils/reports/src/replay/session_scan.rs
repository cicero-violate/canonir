use serde_json::Value;
use std::fs;
use std::io::BufRead;
use std::path::Path;

use crate::replay::tlog_reader::parse_tlog_event;

pub fn find_last_session_offset(tlog_path: &Path) -> Option<u64> {
    let idx_path = tlog_path.with_extension("tlog.idx");
    let data = fs::read_to_string(idx_path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;
    value.get("last_session_offset").and_then(|v| v.as_u64())
}

pub fn find_last_graph_session_offset(tlog_path: &Path) -> Option<u64> {
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
        let mut slice = raw_line.as_str();
        let mut slice_offset = line_start;
        loop {
            if let Some(idx) = slice.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = slice.split_at(idx);
                    if let Some(record) = parse_tlog_event(prefix) {
                        if matches!(record.get("t").and_then(|v| v.as_str()), Some("N") | Some("NODE") | Some("E") | Some("EDGE")) {
                            current_has_graph = true;
                        }
                    }
                    slice_offset = slice_offset.saturating_add(idx as u64);
                    slice = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(slice) {
                match record.get("t").and_then(|v| v.as_str()) {
                    Some("SESSION") => {
                        if current_has_graph {
                            if let Some(off) = current_session_offset {
                                last_with_graph = Some(off);
                            }
                        }
                        current_session_offset = Some(slice_offset);
                        current_has_graph = false;
                    }
                    Some("N") | Some("NODE") | Some("E") | Some("EDGE") => {
                        current_has_graph = true;
                    }
                    _ => {}
                }
            }
            break;
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
        let mut line = raw_line.as_str();
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(record) = parse_tlog_event(prefix) {
                        if matches!(record.get("t").and_then(|v| v.as_str()), Some("N") | Some("NODE"))
                            && matches!(record.get("kind").and_then(|v| v.as_str()), Some("MODULE"))
                        {
                            return true;
                        }
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(line) {
                if matches!(record.get("t").and_then(|v| v.as_str()), Some("N") | Some("NODE"))
                    && matches!(record.get("kind").and_then(|v| v.as_str()), Some("MODULE"))
                {
                    return true;
                }
            }
            break;
        }
    }
    false
}
