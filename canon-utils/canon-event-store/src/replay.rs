use anyhow::Result;
use canon_event::RustcEvent;
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::Path;

use crate::graph_types::{CodeEdge, CodeNode, CodeGraphState};
use crate::reader::{extract_rustc_event, parse_any_event, read_any_events_from_path, read_any_events_from_path_with_start_seq, AnyEvent, detect_tlog_format, TlogFormat};
use crate::session_scan::{find_last_graph_session_offset, find_last_session_offset};

pub fn replay_graph_from_tlog(tlog_path: &Path) -> Result<CodeGraphState> {
    if detect_tlog_format(tlog_path) == TlogFormat::Binary || tlog_path.is_dir() {
        let mut graph = CodeGraphState::default();
        let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
        let events = read_any_events_from_path(tlog_path)?;
        for event in events {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_rustc_event(&canon) {
                    apply_rustc_event_to_graph(kernel, &mut graph, &mut symbol_to_id, true);
                }
            }
        }
        return Ok(graph);
    }
    let mut file = fs::File::open(tlog_path)?;
    let mut stop_after_session = false;
    if let Some(offset) = find_last_graph_session_offset(tlog_path)
        .or_else(|| find_last_session_offset(tlog_path))
    {
        use std::io::Seek;
        use std::io::SeekFrom;
        let _ = file.seek(SeekFrom::Start(offset));
        stop_after_session = true;
    }
    let reader = std::io::BufReader::new(file);
    let mut graph = CodeGraphState::default();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut seen_session = false;

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let line = raw_line.as_str();
        loop {
            if let Some(event) = parse_any_event(line) {
                if let AnyEvent::Canon(canon) = event {
                    if let Some(kernel) = extract_rustc_event(&canon) {
                        if matches!(kernel, RustcEvent::SessionStart { .. }) {
                            if seen_session && stop_after_session {
                                return Ok(graph);
                            }
                            seen_session = true;
                        }
                        apply_rustc_event_to_graph(kernel, &mut graph, &mut symbol_to_id, true);
                    }
                }
            }
            break;
        }
    }

    Ok(graph)
}

pub fn replay_graph_from_tlog_incremental(
    tlog_path: &Path,
    snapshot_path: &Path,
    meta_path: &Path,
) -> Result<CodeGraphState> {
    if detect_tlog_format(tlog_path) == TlogFormat::Binary || tlog_path.is_dir() {
        return replay_graph_from_tlog(tlog_path);
    }
    use crate::snapshot::{load_graph_snapshot, read_snapshot_metadata, snapshot_into_rows};

    let mut graph = CodeGraphState::default();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut base_offset: u64 = 0;
    let mut stop_after_session = false;

    if snapshot_path.exists() && meta_path.exists() {
        if let Ok(meta) = read_snapshot_metadata(meta_path) {
            if meta.version == 2 {
                if let Ok(snapshot) = load_graph_snapshot(snapshot_path) {
                    let (snap_nodes, snap_edges, snap_files) = snapshot_into_rows(snapshot);
                    graph.nodes = snap_nodes;
                    graph.edges = snap_edges;
                    graph.files = snap_files;
                    symbol_to_id = rebuild_symbol_index(&graph.nodes);
                    if graph.nodes.is_empty() && graph.edges.is_empty() {
                        graph = CodeGraphState::default();
                        symbol_to_id.clear();
                        base_offset = 0;
                    } else {
                        base_offset = meta.tlog_offset;
                    }
                } else {
                    base_offset = 0;
                }
            }
        }
    }
    if base_offset == 0 {
        if let Some(offset) = find_last_graph_session_offset(tlog_path) {
            base_offset = offset;
            stop_after_session = true;
        }
    }

    let (_new_offset, _new_events) = replay_events_from_offset(
        tlog_path,
        base_offset,
        &mut graph,
        &mut symbol_to_id,
        stop_after_session,
    )?;

    Ok(graph)
}

pub fn replay_events_from_offset(
    tlog_path: &Path,
    start_offset: u64,
    graph: &mut CodeGraphState,
    symbol_to_id: &mut HashMap<String, u32>,
    stop_after_session: bool,
) -> Result<(u64, u64)> {
    if detect_tlog_format(tlog_path) == TlogFormat::Binary || tlog_path.is_dir() {
        let mut events_added: u64 = 0;
        let events = read_any_events_from_path_with_start_seq(tlog_path, start_offset)?;
        for event in events {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_rustc_event(&canon) {
                    if apply_rustc_event_to_graph(kernel, graph, symbol_to_id, true) {
                        events_added += 1;
                    }
                }
            }
        }
        return Ok((start_offset, events_added));
    }
    let file = fs::File::open(tlog_path)?;
    let metadata_len = file.metadata()?.len();
    let offset = start_offset.min(metadata_len);
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[offset as usize..];
    let mut cursor = 0usize;
    let mut events_added: u64 = 0;
    let mut seen_session = false;

    'outer: while cursor < bytes.len() {
        let line_end = bytes[cursor..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|idx| cursor + idx)
            .unwrap_or(bytes.len());
        let line_bytes = &bytes[cursor..line_end];
        let line = String::from_utf8_lossy(line_bytes);
        let slice = line.as_ref();
        let clear_on_session = true;
        loop {
            if let Some(event) = parse_any_event(slice) {
                if let AnyEvent::Canon(canon) = event {
                    if let Some(kernel) = extract_rustc_event(&canon) {
                        if matches!(kernel, RustcEvent::SessionStart { .. }) {
                            if seen_session && stop_after_session {
                                break 'outer;
                            }
                            seen_session = true;
                        }
                        if apply_rustc_event_to_graph(kernel, graph, symbol_to_id, clear_on_session) {
                            events_added += 1;
                        }
                    }
                }
            }
            break;
        }

        cursor = line_end + 1;
    }

    Ok((metadata_len, events_added))
}

pub fn rebuild_symbol_index(nodes: &[CodeNode]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for node in nodes {
        map.insert(node.symbol.clone(), node.id);
    }
    map
}

pub fn apply_rustc_event_to_graph(
    event: RustcEvent,
    graph: &mut CodeGraphState,
    symbol_to_id: &mut HashMap<String, u32>,
    clear_on_session: bool,
) -> bool {
    match event {
        RustcEvent::SessionStart { .. } => {
            if clear_on_session {
                graph.nodes.clear();
                graph.edges.clear();
                graph.files.clear();
                symbol_to_id.clear();
            }
            true
        }
        RustcEvent::NodeDefined { symbol, kind, file, line, .. }
        | RustcEvent::NodeUpdated { symbol, kind, file, line, .. } => {
            let sym = symbol.as_str();
            let kind = kind.as_str();
            let file = file.as_str();
            let line = Some(line).filter(|v| *v > 0);
            if kind.is_empty() {
                return false;
            }
            if sym.is_empty() && kind != "MODULE" {
                return false;
            }
            if !sym.is_empty() {
                if let Some(&id) = symbol_to_id.get(sym) {
                    if let Some(node) = graph.nodes.get_mut(id as usize) {
                        node.kind = kind.to_string();
                        if !file.is_empty() {
                            let file_id = graph
                                .files
                                .iter()
                                .position(|p| p == file)
                                .map(|idx| idx as u32)
                                .or_else(|| {
                                    graph.files.push(file.to_string());
                                    Some((graph.files.len() - 1) as u32)
                                });
                            node.file_id = file_id;
                        }
                        if line.is_some() {
                            node.line = line;
                        }
                        return true;
                    }
                }
            }
            let file_id = if file.is_empty() {
                None
            } else {
                let file_id = graph.files.iter().position(|p| p == file).map(|idx| idx as u32);
                file_id.or_else(|| {
                    graph.files.push(file.to_string());
                    Some((graph.files.len() - 1) as u32)
                })
            };
            let id = graph.nodes.len() as u32;
            graph.nodes.push(CodeNode {
                id,
                kind: kind.to_string(),
                symbol: sym.to_string(),
                file_id,
                line,
            });
            if !sym.is_empty() {
                symbol_to_id.insert(sym.to_string(), id);
            }
            true
        }
        RustcEvent::EdgeDefined { src, dst, kind } => {
            let src_sym = src.as_str();
            let dst_sym = dst.as_str();
            let kind = kind.as_str();
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            graph.edges.push(CodeEdge {
                src,
                dst,
                kind: kind.to_string(),
            });
            true
        }
        RustcEvent::NodeRemoved { symbol } => {
            let sym = symbol.as_str();
            let Some(&id) = symbol_to_id.get(sym) else {
                return false;
            };
            delete_node(id, graph, symbol_to_id)
        }
        RustcEvent::EdgeRemoved { src, dst, kind } => {
            let src_sym = src.as_str();
            let dst_sym = dst.as_str();
            let kind = kind.as_str();
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            let before = graph.edges.len();
            graph.edges.retain(|e| !(e.src == src && e.dst == dst && e.kind == kind));
            before != graph.edges.len()
        }
        RustcEvent::FileSeen { path } => {
            let path = path.as_str();
            if !path.is_empty() && !graph.files.iter().any(|p| p == path) {
                graph.files.push(path.to_string());
            }
            true
        }
        RustcEvent::WarningCaptured { .. }
        | RustcEvent::PanicCaptured { .. }
        | RustcEvent::CallsiteObserved { .. }
        | RustcEvent::SymbolDefined { .. }
        | RustcEvent::SpanDefined { .. }
        | RustcEvent::CompilationUnitFinished { .. }
        | RustcEvent::InvariantViolation { .. } => true,
    }
}

fn delete_node(
    id: u32,
    graph: &mut CodeGraphState,
    symbol_to_id: &mut HashMap<String, u32>,
) -> bool {
    let idx = id as usize;
    if idx >= graph.nodes.len() {
        return false;
    }
    let last_idx = graph.nodes.len() - 1;
    let removed = graph.nodes.swap_remove(idx);
    if !removed.symbol.is_empty() {
        symbol_to_id.remove(&removed.symbol);
    }
    graph.edges.retain(|e| e.src != id && e.dst != id);
    if idx != last_idx {
        let swapped_id = id;
        let old_last_id = last_idx as u32;
        if let Some(node) = graph.nodes.get_mut(idx) {
            node.id = swapped_id;
            if !node.symbol.is_empty() {
                symbol_to_id.insert(node.symbol.clone(), swapped_id);
            }
        }
        for e in graph.edges.iter_mut() {
            if e.src == old_last_id {
                e.src = swapped_id;
            }
            if e.dst == old_last_id {
                e.dst = swapped_id;
            }
        }
    }
    true
}
