use anyhow::Result;
use memmap2::Mmap;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, Seek, SeekFrom};
use std::path::Path;

use crate::artifacts::snapshot::{load_graph_snapshot, read_snapshot_metadata, snapshot_into_rows};
use crate::graph::graph_types::{EdgeRow, NodeRow};
use crate::graph::graph_builder::apply_event_to_graph;
use crate::graph::graph_builder::rebuild_symbol_index;
use crate::replay::session_scan::{find_last_graph_session_offset, find_last_session_offset};

pub fn replay_graph_from_tlog(tlog_path: &Path) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
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
    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut files: Vec<String> = Vec::new();
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
                        let tag = record.get("t").and_then(|v| v.as_str());
                        if tag == Some("SESSION") {
                            if seen_session && stop_after_session {
                                return Ok((nodes, edges, files));
                            }
                            seen_session = true;
                        }
                        apply_event_to_graph(record, &mut nodes, &mut edges, &mut files, &mut symbol_to_id, true);
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(line) {
                let tag = record.get("t").and_then(|v| v.as_str());
                if tag == Some("SESSION") {
                    if seen_session && stop_after_session {
                        return Ok((nodes, edges, files));
                    }
                    seen_session = true;
                }
                apply_event_to_graph(record, &mut nodes, &mut edges, &mut files, &mut symbol_to_id, true);
            }
            break;
        }
    }

    Ok((nodes, edges, files))
}

pub fn replay_graph_from_tlog_incremental(
    tlog_path: &Path,
    snapshot_path: &Path,
    meta_path: &Path,
) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut base_offset: u64 = 0;

    let mut stop_after_session = false;
    if snapshot_path.exists() && meta_path.exists() {
        if let Ok(meta) = read_snapshot_metadata(meta_path) {
            if meta.version == 2 {
                if let Ok(snapshot) = load_graph_snapshot(snapshot_path) {
                    let (snap_nodes, snap_edges, snap_files) = snapshot_into_rows(snapshot);
                    nodes = snap_nodes;
                    edges = snap_edges;
                    files = snap_files;
                    symbol_to_id = rebuild_symbol_index(&nodes);
                    if nodes.is_empty() && edges.is_empty() {
                        nodes.clear();
                        edges.clear();
                        files.clear();
                        symbol_to_id.clear();
                        base_offset = 0;
                    } else {
                        base_offset = meta.tlog_offset;
                    }
                } else {
                    // Corrupt snapshot: fall back to full tlog replay.
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
        &mut nodes,
        &mut edges,
        &mut files,
        &mut symbol_to_id,
        stop_after_session,
    )?;

    Ok((nodes, edges, files))
}

pub fn parse_tlog_event(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

pub fn replay_events_from_offset(
    tlog_path: &Path,
    start_offset: u64,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    files: &mut Vec<String>,
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
                        let tag = record.get("t").and_then(|v| v.as_str());
                        if tag == Some("SESSION") {
                            if seen_session && stop_after_session {
                                break 'outer;
                            }
                            seen_session = true;
                        }
                        if apply_event_to_graph(record, nodes, edges, files, symbol_to_id, clear_on_session) {
                            events_added += 1;
                        }
                    }
                    slice = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_event(slice) {
                let tag = record.get("t").and_then(|v| v.as_str());
                if tag == Some("SESSION") {
                    if seen_session && stop_after_session {
                        break 'outer;
                    }
                    seen_session = true;
                }
                if apply_event_to_graph(record, nodes, edges, files, symbol_to_id, clear_on_session) {
                    events_added += 1;
                }
            }
            break;
        }

        cursor = line_end + 1;
    }

    Ok((metadata_len, events_added))
}

fn remove_node_by_id(
    id: u32,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    symbol_to_id: &mut HashMap<String, u32>,
) -> bool {
    let idx = id as usize;
    if idx >= nodes.len() {
        return false;
    }
    let last_idx = nodes.len() - 1;
    let removed = nodes.swap_remove(idx);
    if !removed.symbol.is_empty() {
        symbol_to_id.remove(&removed.symbol);
    }
    edges.retain(|e| e.src != id && e.dst != id);
    if idx != last_idx {
        let swapped_id = id;
        let old_last_id = last_idx as u32;
        if let Some(node) = nodes.get_mut(idx) {
            node.id = swapped_id;
            if !node.symbol.is_empty() {
                symbol_to_id.insert(node.symbol.clone(), swapped_id);
            }
        }
        for e in edges.iter_mut() {
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

fn normalize_graph_rows(
    mut nodes: Vec<NodeRow>,
    mut edges: Vec<EdgeRow>,
    files: Vec<String>,
) -> (Vec<NodeRow>, Vec<EdgeRow>, Vec<String>) {
    let mut indexed_files: Vec<(u32, String)> = files
        .into_iter()
        .enumerate()
        .map(|(idx, path)| (idx as u32, path))
        .collect();
    indexed_files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut file_id_map: HashMap<u32, u32> = HashMap::new();
    let mut normalized_files: Vec<String> = Vec::with_capacity(indexed_files.len());
    for (new_idx, (old_idx, path)) in indexed_files.into_iter().enumerate() {
        file_id_map.insert(old_idx, new_idx as u32);
        normalized_files.push(path);
    }

    for node in &mut nodes {
        if let Some(old_id) = node.file_id {
            if let Some(&new_id) = file_id_map.get(&old_id) {
                node.file_id = Some(new_id);
            }
        }
    }

    nodes.sort_by_key(|n| n.id);
    edges.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then_with(|| a.dst.cmp(&b.dst))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    (nodes, edges, normalized_files)
}


fn write_graph_artifacts(
    out_dir: &Path,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
) -> Result<(Vec<EdgeRow>, Vec<(u32, u32)>)> {
    let mut cfg = build_cfg_edges(nodes, edges);
    cfg.sort_by(|a, b| {
        a.src
            .cmp(&b.src)
            .then_with(|| a.dst.cmp(&b.dst))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    let mut callgraph = build_callgraph_edges(nodes, edges);
    callgraph.sort_unstable();

    let (modulegraph, module_nodes) = build_modulegraph(nodes, files);
    let mut modulegraph = modulegraph;
    modulegraph.sort_unstable();

    let mut typegraph = build_typegraph_edges(nodes, edges);
    typegraph.sort_by(|a, b| {
        a.0
            .cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    write_cfg_csv(out_dir, &cfg)?;
    write_callgraph_csv(out_dir, &callgraph, nodes, files)?;
    write_modulegraph_csv(out_dir, &modulegraph, &module_nodes)?;
    write_typegraph_csv(out_dir, &typegraph, nodes, files)?;

    Ok((cfg, callgraph))
}
