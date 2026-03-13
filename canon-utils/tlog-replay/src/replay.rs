use anyhow::Result;
use canon_types::TlogEvent;
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::Path;

use crate::graph_types::{EdgeRow, NodeRow, ReplayGraph};
use crate::reader::parse_tlog_event;
use crate::session_scan::{find_last_graph_session_offset, find_last_session_offset};

pub fn replay_graph_from_tlog(tlog_path: &Path) -> Result<ReplayGraph> {
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
    let mut graph = ReplayGraph::default();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut seen_session = false;

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let mut line = raw_line.as_str();
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(record) = parse_tlog_event(prefix) {
                        if matches!(record, TlogEvent::Session { .. }) {
                            if seen_session && stop_after_session {
                                return Ok(graph);
                            }
                            seen_session = true;
                        }
                        apply_event_to_graph(record, &mut graph, &mut symbol_to_id, true);
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(line) {
                if matches!(record, TlogEvent::Session { .. }) {
                    if seen_session && stop_after_session {
                        return Ok(graph);
                    }
                    seen_session = true;
                }
                apply_event_to_graph(record, &mut graph, &mut symbol_to_id, true);
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
) -> Result<ReplayGraph> {
    use crate::snapshot::{load_graph_snapshot, read_snapshot_metadata, snapshot_into_rows};

    let mut graph = ReplayGraph::default();
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
                        graph = ReplayGraph::default();
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
    graph: &mut ReplayGraph,
    symbol_to_id: &mut HashMap<String, u32>,
    stop_after_session: bool,
) -> Result<(u64, u64)> {
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
        let mut slice = line.as_ref();
        let clear_on_session = true;
        loop {
            if let Some(idx) = slice.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = slice.split_at(idx);
                    if let Some(record) = parse_tlog_event(prefix) {
                        if matches!(record, TlogEvent::Session { .. }) {
                            if seen_session && stop_after_session {
                                break 'outer;
                            }
                            seen_session = true;
                        }
                        if apply_event_to_graph(record, graph, symbol_to_id, clear_on_session) {
                            events_added += 1;
                        }
                    }
                    slice = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(slice) {
                if matches!(record, TlogEvent::Session { .. }) {
                    if seen_session && stop_after_session {
                        break 'outer;
                    }
                    seen_session = true;
                }
                if apply_event_to_graph(record, graph, symbol_to_id, clear_on_session) {
                    events_added += 1;
                }
            }
            break;
        }

        cursor = line_end + 1;
    }

    Ok((metadata_len, events_added))
}

pub fn rebuild_symbol_index(nodes: &[NodeRow]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for node in nodes {
        map.insert(node.symbol.clone(), node.id);
    }
    map
}

fn apply_event_to_graph(
    event: TlogEvent,
    graph: &mut ReplayGraph,
    symbol_to_id: &mut HashMap<String, u32>,
    clear_on_session: bool,
) -> bool {
    match event {
        TlogEvent::Session { .. } => {
            if clear_on_session {
                graph.nodes.clear();
                graph.edges.clear();
                graph.files.clear();
                symbol_to_id.clear();
            }
            true
        }
        TlogEvent::Node { sym, kind, file, line, .. }
        | TlogEvent::NodeUpdate { sym, kind, file, line, .. } => {
            let sym = sym.as_str();
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
            graph.nodes.push(NodeRow {
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
        TlogEvent::Edge { src, dst, kind } => {
            let src_sym = src.as_str();
            let dst_sym = dst.as_str();
            let kind = kind.as_str();
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            graph.edges.push(EdgeRow {
                src,
                dst,
                kind: kind.to_string(),
            });
            true
        }
        TlogEvent::NodeRemove { sym } => {
            let sym = sym.as_str();
            let Some(&id) = symbol_to_id.get(sym) else {
                return false;
            };
            delete_node(id, graph, symbol_to_id)
        }
        TlogEvent::EdgeRemove { src, dst, kind } => {
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
        TlogEvent::File { path } => {
            let path = path.as_str();
            if !path.is_empty() && !graph.files.iter().any(|p| p == path) {
                graph.files.push(path.to_string());
            }
            true
        }
        TlogEvent::Warning { .. }
        | TlogEvent::Panic { .. }
        | TlogEvent::Callsite { .. }
        | TlogEvent::Symbol { .. }
        | TlogEvent::Span { .. }
        | TlogEvent::CompilationUnitFinished { .. } => true,
    }
}

fn delete_node(
    id: u32,
    graph: &mut ReplayGraph,
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
