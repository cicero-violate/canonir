use anyhow::{anyhow, Result};
use serde_json::Value;
use serde::{Deserialize, Serialize};
use rkyv::{
    Archive,
    Deserialize as RkyvDeserialize,
    Serialize as RkyvSerialize,
};
use rkyv::Infallible;
use rkyv::ser::Serializer;
use memmap2::Mmap;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use algorithms::graph::csr::Csr;
#[cfg(feature = "cuda")]
use algorithms::graph::scc_gpu::scc_gpu;
#[cfg(feature = "cuda")]
use algorithms::graph::reachability::{reachability_gpu, reachability_batched_gpu};

#[derive(Debug, Clone)]
struct NodeRow {
    id: u32,
    kind: String,
    symbol: String,
    file_id: Option<u32>,
    line: Option<u32>,
}

#[derive(Debug, Clone)]
struct EdgeRow {
    src: u32,
    dst: u32,
    kind: String,
}

#[derive(Serialize)]
struct BranchComplexityEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_block_count: usize,
    score: usize,
}

#[derive(Serialize)]
struct CallgraphCentralityEntry {
    symbol: String,
    file: String,
    caller_count: usize,
    callee_count: usize,
    centrality_score: usize,
}

#[derive(Serialize)]
struct DeadCodeEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    reason: String,
}

#[derive(Serialize)]
struct DependencyCycleEntry {
    cycle_id: usize,
    nodes: Vec<String>,
    files: Vec<String>,
    cycle_length: usize,
}

#[derive(Serialize)]
struct StructuralHotspotEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_count: usize,
    duplicate_blocks: usize,
    callers: Vec<String>,
    score: usize,
}

#[derive(Serialize)]
struct DataflowFanoutEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    outgoing_edges: usize,
    mutation_edges: usize,
    io_edges: usize,
}

#[derive(Serialize)]
struct BranchPressureEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    branch_nodes: usize,
    branch_pressure: usize,
}

#[derive(Serialize)]
struct MergeCandidateEntry {
    function: String,
    branch_block: u32,
    successors: Vec<u32>,
    candidate_blocks: Vec<u32>,
}

#[derive(Serialize)]
struct ReachabilityEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    reachable_blocks: usize,
    total_blocks: usize,
    reachable_ratio: f64,
}

#[derive(Serialize)]
struct PathRedundancyEntry {
    symbol: String,
    file: String,
    line: Option<u32>,
    paths_total: usize,
    paths_unique: usize,
    redundancy_ratio: f64,
}

pub fn generate_reports(output_dir: &Path, out_dir: &Path) -> Result<()> {
    let nodes = read_nodes_csv(output_dir.join("nodes.csv"))?;
    let edges = read_edges_csv(output_dir.join("edges.csv"))?;
    let files = read_files_txt(output_dir.join("files.txt"))?;
    let _symbols_json = fs::read_to_string(output_dir.join("symbols.json"))
        .map_err(|e| anyhow!("failed to read symbols.json: {e}"))?;
    let reports_dir = out_dir.join("reports");
    generate_reports_from_parts(nodes, edges, files, out_dir, &reports_dir)
}

pub fn generate_reports_from_tlog(tlog_path: &Path, out_dir: &Path) -> Result<()> {
    let snapshot_path = out_dir.join("graph_snapshot.bin");
    let meta_path = out_dir.join("snapshot.meta.json");
    let graph_bin_path = out_dir.join("graph.bin");
    let (nodes, edges, files) = if graph_bin_path.exists() && is_graph_bin_fresh(&graph_bin_path, tlog_path) {
        load_graph_bin(&graph_bin_path)?
    } else {
        read_tlog_graph_incremental(tlog_path, &snapshot_path, &meta_path)?
    };
    if !graph_bin_path.exists() || !is_graph_bin_fresh(&graph_bin_path, tlog_path) {
        write_graph_bin(&graph_bin_path, &nodes, &edges, &files)?;
        write_kernel_snapshot(&snapshot_path, &nodes, &edges, &files)?;
        let meta = SnapshotMeta {
            tlog_offset: tlog_path.metadata().map(|m| m.len()).unwrap_or(0),
            event_count: (nodes.len() + edges.len()) as u64,
            created_at: current_timestamp(),
            version: 2,
        };
        write_snapshot_meta(&meta_path, &meta)?;
    }
    let reports_dir = out_dir.join("reports");
    generate_reports_from_parts(nodes, edges, files, out_dir, &reports_dir)?;
    if let Ok(cache) = load_and_update_graph_cache(tlog_path, &reports_dir) {
        let (modulegraph, module_nodes) = build_modulegraph_from_cache(&cache);
        write_modulegraph_csv(out_dir, &modulegraph, &module_nodes)?;
        if !cache.type_nodes.is_empty() || !cache.type_edges.is_empty() {
            let (typegraph, type_nodes) = build_typegraph_from_cache(&cache);
            write_typegraph_csv_from_cache(out_dir, &typegraph, &type_nodes)?;
        }
    }
    Ok(())
}

fn generate_reports_from_parts(
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    files: Vec<String>,
    graph_dir: &Path,
    reports_dir: &Path,
) -> Result<()> {
    fs::create_dir_all(graph_dir)?;
    fs::create_dir_all(reports_dir)?;
    let (cfg, callgraph) = write_graph_artifacts(graph_dir, &nodes, &edges, &files)?;

    let node_map: HashMap<u32, NodeRow> = nodes.iter().map(|n| (n.id, n.clone())).collect();

    let mut file_map: HashMap<u32, String> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(idx as u32, path.clone());
    }

    let cfg_out = build_cfg_out(&cfg);
    let cfg_in = build_cfg_in(&cfg);

    let block_owner = build_block_owner(&nodes, &edges);
    let block_effect_sig = build_block_effect_signatures(&edges, &node_map);

    // Build callgraph CSR once — shared by GPU SCC, GPU reachability, dead code
    let (cg_csr, cg_id_to_local, cg_local_to_id) = build_callgraph_csr(&callgraph);

    // Parallel dispatch — independent reports run concurrently
    let results: Vec<Result<()>> = [
        "branch_complexity",
        "callgraph_centrality",
        "dead_code",
        "dependency_cycles",
        "structural_hotspots",
        "dataflow_fanout",
        "branch_pressure",
        "merge_candidates",
        "reachability",
        "path_redundancy",
    ]
    .into_par_iter()
    .map(|report| match report {
        "branch_complexity" => {
            let r = build_branch_complexity(
                &nodes,
                &node_map,
                &file_map,
                &cfg_out,
                &cfg_in,
                &block_effect_sig,
            );
            write_report(&reports_dir.join("branch_complexity_report.json"), &r)
        }
        "callgraph_centrality" => {
            let r = build_callgraph_centrality(&callgraph, &node_map, &file_map);
            write_report(&reports_dir.join("callgraph_centrality_report.json"), &r)
        }
        "dead_code" => {
            let r = build_dead_code_gpu(
                &nodes,
                &node_map,
                &file_map,
                &edges,
                &cfg_out,
                &cfg_in,
                &callgraph,
                &block_owner,
                &cg_csr,
                &cg_id_to_local,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("dead_code_report.json"), &r)
        }
        "dependency_cycles" => {
            let r = build_dependency_cycles_gpu(
                &callgraph,
                &node_map,
                &file_map,
                &cg_csr,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("dependency_cycle_report.json"), &r)
        }
        "structural_hotspots" => {
            let r = build_structural_hotspots(
                &nodes,
                &node_map,
                &file_map,
                &callgraph,
                &cfg_out,
                &cfg_in,
                &block_owner,
                &block_effect_sig,
            );
            write_report(&reports_dir.join("structural_hotspots_report.json"), &r)
        }
        "dataflow_fanout" => {
            let r = build_dataflow_fanout(&nodes, &node_map, &file_map, &edges, &block_owner);
            write_report(&reports_dir.join("dataflow_fanout_report.json"), &r)
        }
        "branch_pressure" => {
            let r = build_branch_pressure(&block_owner, &node_map, &file_map, &cfg_out);
            write_report(&reports_dir.join("branch_pressure_report.json"), &r)
        }
        "merge_candidates" => {
            let r = build_merge_candidates(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&reports_dir.join("merge_candidates_report.json"), &r)
        }
        "reachability" => {
            let r = build_reachability_report_gpu(
                &cfg_out,
                &block_owner,
                &node_map,
                &file_map,
                &cg_csr,
                &cg_id_to_local,
                &cg_local_to_id,
            );
            write_report(&reports_dir.join("reachability_report.json"), &r)
        }
        "path_redundancy" => {
            let r = build_path_redundancy(&cfg_out, &block_owner, &node_map, &file_map);
            write_report(&reports_dir.join("path_redundancy_report.json"), &r)
        }
        _ => Ok(()),
    })
    .collect();

    for result in results {
        result?;
    }

    Ok(())
}

fn read_tlog_graph(tlog_path: &Path) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
    let mut file = fs::File::open(tlog_path)?;
    if let Some(offset) = read_last_session_offset(tlog_path) {
        use std::io::Seek;
        use std::io::SeekFrom;
        let _ = file.seek(SeekFrom::Start(offset));
    }
    let reader = std::io::BufReader::new(file);
    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let mut line = raw_line.as_str();
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(record) = parse_tlog_line(prefix) {
                        apply_tlog_record(record, &mut nodes, &mut edges, &mut files, &mut symbol_to_id, true);
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_line(line) {
                apply_tlog_record(record, &mut nodes, &mut edges, &mut files, &mut symbol_to_id, true);
            }
            break;
        }
    }

    Ok((nodes, edges, files))
}

fn read_tlog_graph_incremental(
    tlog_path: &Path,
    snapshot_path: &Path,
    meta_path: &Path,
) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut symbol_to_id: HashMap<String, u32> = HashMap::new();
    let mut base_offset: u64 = 0;

    if snapshot_path.exists() && meta_path.exists() {
        if let Ok(meta) = read_snapshot_meta(meta_path) {
            if meta.version == 2 {
                if let Ok(snapshot) = load_kernel_snapshot(snapshot_path) {
                    let (snap_nodes, snap_edges, snap_files) = snapshot_into_rows(snapshot);
                    nodes = snap_nodes;
                    edges = snap_edges;
                    files = snap_files;
                    symbol_to_id = rebuild_symbol_index(&nodes);
                    base_offset = meta.tlog_offset;
                }
            }
        }
    }

    let (_new_offset, _new_events) = replay_tlog_from_offset(
        tlog_path,
        base_offset,
        &mut nodes,
        &mut edges,
        &mut files,
        &mut symbol_to_id,
    )?;

    Ok((nodes, edges, files))
}

fn parse_tlog_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn replay_tlog_from_offset(
    tlog_path: &Path,
    start_offset: u64,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    files: &mut Vec<String>,
    symbol_to_id: &mut HashMap<String, u32>,
) -> Result<(u64, u64)> {
    let file = fs::File::open(tlog_path)?;
    let metadata_len = file.metadata()?.len();
    let offset = start_offset.min(metadata_len);
    let mmap = unsafe { Mmap::map(&file)? };
    let bytes = &mmap[offset as usize..];
    let mut cursor = 0usize;
    let mut events_added: u64 = 0;

    while cursor < bytes.len() {
        let line_end = bytes[cursor..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|idx| cursor + idx)
            .unwrap_or(bytes.len());
        let line_bytes = &bytes[cursor..line_end];
        let line = String::from_utf8_lossy(line_bytes);
        let mut slice = line.as_ref();
        let clear_on_session = start_offset == 0;
        loop {
            if let Some(idx) = slice.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = slice.split_at(idx);
                    if let Some(record) = parse_tlog_line(prefix) {
                        if apply_tlog_record(record, nodes, edges, files, symbol_to_id, clear_on_session) {
                            events_added += 1;
                        }
                    }
                    slice = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_line(slice) {
                if apply_tlog_record(record, nodes, edges, files, symbol_to_id, clear_on_session) {
                    events_added += 1;
                }
            }
            break;
        }

        cursor = line_end + 1;
    }

    Ok((metadata_len, events_added))
}

fn read_last_session_offset(tlog_path: &Path) -> Option<u64> {
    let idx_path = tlog_path.with_extension("tlog.idx");
    let data = fs::read_to_string(idx_path).ok()?;
    let value: Value = serde_json::from_str(&data).ok()?;
    value.get("last_session_offset").and_then(|v| v.as_u64())
}

fn apply_tlog_record(
    value: Value,
    nodes: &mut Vec<NodeRow>,
    edges: &mut Vec<EdgeRow>,
    files: &mut Vec<String>,
    symbol_to_id: &mut HashMap<String, u32>,
    clear_on_session: bool,
) -> bool {
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return false;
    };
    match tag {
        "SESSION" => {
            if clear_on_session {
                nodes.clear();
                edges.clear();
                files.clear();
                symbol_to_id.clear();
            }
            true
        }
        "N" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let line = value.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
            if (sym.is_empty() && kind != "MODULE") || file.is_empty() {
                return false;
            }
            let file_id = files.iter().position(|p| p == file).map(|idx| idx as u32);
            let file_id = file_id.or_else(|| {
                files.push(file.to_string());
                Some((files.len() - 1) as u32)
            });
            let id = nodes.len() as u32;
            nodes.push(NodeRow {
                id,
                kind: kind.to_string(),
                symbol: sym.to_string(),
                file_id,
                line,
            });
            symbol_to_id.insert(sym.to_string(), id);
            true
        }
        "E" => {
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let Some(&src) = symbol_to_id.get(src_sym) else {
                return false;
            };
            let Some(&dst) = symbol_to_id.get(dst_sym) else {
                return false;
            };
            edges.push(EdgeRow {
                src,
                dst,
                kind: kind.to_string(),
            });
            true
        }
        "F" => {
            let path = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if !path.is_empty() && !files.iter().any(|p| p == path) {
                files.push(path.to_string());
            }
            true
        }
        _ => false,
    }
}


fn write_graph_artifacts(
    out_dir: &Path,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
) -> Result<(Vec<EdgeRow>, Vec<(u32, u32)>)> {
    let cfg = build_cfg_edges(nodes, edges);
    let callgraph = build_callgraph_edges(nodes, edges);
    let (modulegraph, module_nodes) = build_modulegraph(nodes, files);
    let typegraph = build_typegraph_edges(nodes, edges);

    write_cfg_csv(out_dir, &cfg)?;
    write_callgraph_csv(out_dir, &callgraph, nodes, files)?;
    write_modulegraph_csv(out_dir, &modulegraph, &module_nodes)?;
    write_typegraph_csv(out_dir, &typegraph, nodes, files)?;

    Ok((cfg, callgraph))
}

fn build_cfg_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<EdgeRow> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = Vec::new();
    for edge in edges {
        if edge.kind != "FLOW" && edge.kind != "UNWIND" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") && dst_kind == Some(&"BASIC_BLOCK") {
            out.push(edge.clone());
        }
    }
    out
}

fn build_callgraph_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<(u32, u32)> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut callsite_to_block: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut block_to_fn: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();

    for edge in edges {
        if edge.kind != "HAS_BLOCK" {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        if src_kind == Some(&"BASIC_BLOCK") && dst_kind == Some(&"CALL_SITE") {
            callsite_to_block.entry(edge.dst).or_default().insert(edge.src);
        } else if matches!(src_kind, Some(&"FUNCTION" | &"METHOD")) && dst_kind == Some(&"BASIC_BLOCK") {
            block_to_fn.entry(edge.dst).or_default().insert(edge.src);
        }
    }

    for edge in edges {
        if edge.kind != "CALL" {
            continue;
        }
        let callee_kind = id_to_kind.get(&edge.dst);
        if !matches!(callee_kind, Some(&"FUNCTION" | &"METHOD")) {
            continue;
        }
        if let Some(blocks) = callsite_to_block.get(&edge.src) {
            for block in blocks {
                if let Some(callers) = block_to_fn.get(block) {
                    for caller in callers {
                        seen.insert((*caller, edge.dst));
                    }
                }
            }
        }
    }

    seen.into_iter().collect()
}

#[derive(Clone)]
struct ModuleNode {
    id: u32,
    symbol: String,
    file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GraphCache {
    last_offset: u64,
    module_files: BTreeMap<String, String>,
    type_nodes: BTreeMap<String, TypeNodeCache>,
    type_edges: BTreeSet<TypeEdgeCache>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TypeNodeCache {
    kind: String,
    file: String,
    line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct TypeEdgeCache {
    src: String,
    dst: String,
    rel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotMeta {
    tlog_offset: u64,
    event_count: u64,
    created_at: u64,
    #[serde(default)]
    version: u32,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
struct KernelSnapshot {
    nodes: Vec<KernelSnapshotNode>,
    edges: Vec<KernelSnapshotEdge>,
    files: Vec<String>,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
struct KernelSnapshotNode {
    kind: String,
    symbol: String,
    file: String,
    line: u32,
    column: u32,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
struct KernelSnapshotEdge {
    src_symbol: String,
    src_kind: String,
    dst_symbol: String,
    dst_kind: String,
    kind: String,
}

fn build_modulegraph(nodes: &[NodeRow], files: &[String]) -> (Vec<(u32, u32)>, Vec<ModuleNode>) {
    let mut module_files: BTreeMap<String, String> = BTreeMap::new();

    for node in nodes {
        if node.kind == "MODULE" {
            let file = node
                .file_id
                .and_then(|id| files.get(id as usize))
                .cloned()
                .unwrap_or_default();
            module_files
                .entry(node.symbol.clone())
                .or_insert(file);
        }
        if !node.symbol.is_empty() {
            let file = node
                .file_id
                .and_then(|id| files.get(id as usize))
                .cloned()
                .unwrap_or_default();
            for module_sym in module_prefixes(&node.symbol) {
                module_files.entry(module_sym).or_insert(file.clone());
            }
        }
    }

    if !module_files.contains_key("") {
        module_files.insert("".to_string(), String::new());
    }

    let mut module_nodes: Vec<ModuleNode> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, file) in module_files.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        module_nodes.push(ModuleNode {
            id,
            symbol: if symbol.is_empty() { "crate".to_string() } else { symbol.clone() },
            file: file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for symbol in module_files.keys() {
        if symbol.is_empty() {
            continue;
        }
        let parent_symbol = match symbol.rsplit_once("::") {
            Some((parent, _child)) => parent,
            None => "",
        };
        let Some(&parent_id) = symbol_to_id.get(parent_symbol) else { continue };
        let Some(&child_id) = symbol_to_id.get(symbol) else { continue };
        edges.insert((parent_id, child_id));
    }

    (edges.into_iter().collect(), module_nodes)
}

fn module_prefixes(symbol: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for (idx, part) in symbol.split("::").enumerate() {
        if idx > 0 {
            cur.push_str("::");
        }
        cur.push_str(part);
        out.push(cur.clone());
    }
    out
}

fn load_and_update_graph_cache(tlog_path: &Path, reports_dir: &Path) -> Result<GraphCache> {
    fs::create_dir_all(reports_dir)?;
    let cache_path = reports_dir.join(".graph_cache.json");
    let mut cache = if cache_path.exists() {
        let data = fs::read_to_string(&cache_path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        GraphCache::default()
    };

    let mut file = fs::File::open(tlog_path)?;
    let metadata_len = file.metadata()?.len();
    if cache.last_offset > metadata_len {
        cache.last_offset = 0;
    }
    file.seek(SeekFrom::Start(cache.last_offset))?;
    let reader = std::io::BufReader::new(file);

    for raw_line in reader.lines() {
        let raw_line = raw_line?;
        let mut line = raw_line.as_str();
        loop {
            if let Some(idx) = line.find("{\"t\":\"SESSION\"") {
                if idx > 0 {
                    let (prefix, suffix) = line.split_at(idx);
                    if let Some(record) = parse_tlog_line(prefix) {
                        apply_cache_record(record, &mut cache);
                    }
                    line = suffix;
                    continue;
                }
            }
            if let Some(record) = parse_tlog_line(line) {
                apply_cache_record(record, &mut cache);
            }
            break;
        }
    }

    cache.last_offset = metadata_len;
    fs::write(&cache_path, serde_json::to_string(&cache)?)?;
    Ok(cache)
}

fn apply_cache_record(value: Value, cache: &mut GraphCache) {
    let Some(tag) = value.get("t").and_then(|v| v.as_str()) else {
        return;
    };
    match tag {
        "N" => {
            let sym = value.get("sym").and_then(|v| v.as_str()).unwrap_or("");
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let file = value.get("file").and_then(|v| v.as_str()).unwrap_or("");
            let line = value.get("line").and_then(|v| v.as_u64()).map(|v| v as u32);
            if (sym.is_empty() && kind != "MODULE") {
                return;
            }

            if !sym.is_empty() {
                for module_sym in module_prefixes(sym) {
                    cache
                        .module_files
                        .entry(module_sym)
                        .or_insert_with(|| file.to_string());
                }
            } else if kind == "MODULE" {
                cache
                    .module_files
                    .entry(sym.to_string())
                    .or_insert_with(|| file.to_string());
            }

            let type_kinds = ["STRUCT", "ENUM", "TRAIT", "IMPL", "TYPE"];
            if type_kinds.contains(&kind) && !sym.is_empty() {
                cache.type_nodes.entry(sym.to_string()).or_insert(TypeNodeCache {
                    kind: kind.to_string(),
                    file: file.to_string(),
                    line,
                });
            }
        }
        "E" => {
            let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !rel_kinds.contains(&kind) {
                return;
            }
            let src_sym = value.get("src").and_then(|v| v.as_str()).unwrap_or("");
            let dst_sym = value.get("dst").and_then(|v| v.as_str()).unwrap_or("");
            if src_sym.is_empty() || dst_sym.is_empty() {
                return;
            }
            cache.type_edges.insert(TypeEdgeCache {
                src: src_sym.to_string(),
                dst: dst_sym.to_string(),
                rel: kind.to_string(),
            });
        }
        _ => {}
    }
}

fn rebuild_symbol_index(nodes: &[NodeRow]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for node in nodes {
        map.insert(node.symbol.clone(), node.id);
    }
    map
}

fn read_snapshot_meta(path: &Path) -> Result<SnapshotMeta> {
    let data = fs::read_to_string(path)?;
    let meta = serde_json::from_str(&data)?;
    Ok(meta)
}

fn write_snapshot_meta(path: &Path, meta: &SnapshotMeta) -> Result<()> {
    let data = serde_json::to_string_pretty(meta)?;
    fs::write(path, data)?;
    Ok(())
}

fn load_kernel_snapshot(path: &Path) -> Result<KernelSnapshot> {
    let data = fs::read(path)?;
    let archived = unsafe { rkyv::archived_root::<KernelSnapshot>(&data) };
    let snapshot: KernelSnapshot = archived
        .deserialize(&mut Infallible)
        .map_err(|e| anyhow!("snapshot deserialize failed: {e}"))?;
    Ok(snapshot)
}

fn write_kernel_snapshot(
    path: &Path,
    nodes: &[NodeRow],
    edges: &[EdgeRow],
    files: &[String],
) -> Result<()> {
    let mut nodes_out: Vec<KernelSnapshotNode> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let file = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .cloned()
            .unwrap_or_default();
        nodes_out.push(KernelSnapshotNode {
            kind: node.kind.clone(),
            symbol: node.symbol.clone(),
            file,
            line: node.line.unwrap_or(0),
            column: 0,
        });
    }

    let mut id_to_kind: HashMap<u32, (&str, &str)> = HashMap::new();
    for node in nodes {
        id_to_kind.insert(node.id, (node.symbol.as_str(), node.kind.as_str()));
    }

    let mut edges_out: Vec<KernelSnapshotEdge> = Vec::with_capacity(edges.len());
    for edge in edges {
        let (src_sym, src_kind) = id_to_kind
            .get(&edge.src)
            .copied()
            .unwrap_or(("", "UNKNOWN"));
        let (dst_sym, dst_kind) = id_to_kind
            .get(&edge.dst)
            .copied()
            .unwrap_or(("", "UNKNOWN"));
        edges_out.push(KernelSnapshotEdge {
            src_symbol: src_sym.to_string(),
            src_kind: src_kind.to_string(),
            dst_symbol: dst_sym.to_string(),
            dst_kind: dst_kind.to_string(),
            kind: edge.kind.clone(),
        });
    }

    let snapshot = KernelSnapshot {
        nodes: nodes_out,
        edges: edges_out,
        files: files.to_vec(),
    };

    let mut serializer = rkyv::ser::serializers::AllocSerializer::<256>::default();
    serializer
        .serialize_value(&snapshot)
        .map_err(|e| anyhow!("snapshot serialize failed: {e}"))?;
    let buf = serializer.into_serializer().into_inner();
    fs::write(path, buf)?;
    Ok(())
}

fn is_graph_bin_fresh(graph_bin: &Path, tlog_path: &Path) -> bool {
    let tlog_idx = tlog_path.with_extension("tlog.idx");
    let bin_meta = graph_bin.metadata().and_then(|m| m.modified());
    let idx_meta = tlog_idx.metadata().and_then(|m| m.modified());
    match (bin_meta, idx_meta) {
        (Ok(bin), Ok(idx)) => bin >= idx,
        _ => false,
    }
}

fn write_graph_bin(path: &Path, nodes: &[NodeRow], edges: &[EdgeRow], files: &[String]) -> Result<()> {
    const MAGIC: &[u8; 4] = b"CGBN";
    const VERSION: u32 = 1;
    const HEADER_SIZE: usize = 32;
    const NODE_RECORD_SIZE: usize = 21;
    const EDGE_RECORD_SIZE: usize = 9;
    const NO_FILE_ID: u32 = u32::MAX;
    const NO_LINE: u32 = u32::MAX;

    let n_nodes = nodes.len() as u32;
    let n_edges = edges.len() as u32;
    let n_files = files.len() as u32;

    let mut file_index: HashMap<&str, u32> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_index.insert(path.as_str(), idx as u32);
    }

    let mut string_table: Vec<u8> = Vec::new();
    let mut string_offsets: HashMap<&str, (u32, u32)> = HashMap::new();

    for path in files {
        let offset = string_table.len() as u32;
        let bytes = path.as_bytes();
        string_table.extend_from_slice(bytes);
        string_table.push(0);
        string_offsets.insert(path.as_str(), (offset, bytes.len() as u32));
    }

    for node in nodes {
        if string_offsets.contains_key(node.symbol.as_str()) {
            continue;
        }
        let offset = string_table.len() as u32;
        let bytes = node.symbol.as_bytes();
        string_table.extend_from_slice(bytes);
        string_table.push(0);
        string_offsets.insert(node.symbol.as_str(), (offset, bytes.len() as u32));
    }

    let str_table_offset = HEADER_SIZE as u32
        + n_nodes
            .checked_mul(NODE_RECORD_SIZE as u32)
            .ok_or_else(|| anyhow!("graph.bin node section too large"))?
        + n_edges
            .checked_mul(EDGE_RECORD_SIZE as u32)
            .ok_or_else(|| anyhow!("graph.bin edge section too large"))?;

    let mut out = Vec::with_capacity(
        HEADER_SIZE + (n_nodes as usize * NODE_RECORD_SIZE) + (n_edges as usize * EDGE_RECORD_SIZE) + string_table.len(),
    );

    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&n_nodes.to_le_bytes());
    out.extend_from_slice(&n_edges.to_le_bytes());
    out.extend_from_slice(&n_files.to_le_bytes());
    out.extend_from_slice(&str_table_offset.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);

    for node in nodes {
        let (sym_off, sym_len) = string_offsets
            .get(node.symbol.as_str())
            .copied()
            .unwrap_or((0, 0));
        let file_id = node
            .file_id
            .and_then(|id| files.get(id as usize))
            .and_then(|p| file_index.get(p.as_str()))
            .copied()
            .unwrap_or(NO_FILE_ID);
        out.extend_from_slice(&node.id.to_le_bytes());
        out.push(node_kind_code(node.kind.as_str()));
        out.extend_from_slice(&file_id.to_le_bytes());
        out.extend_from_slice(&node.line.unwrap_or(NO_LINE).to_le_bytes());
        out.extend_from_slice(&sym_off.to_le_bytes());
        out.extend_from_slice(&sym_len.to_le_bytes());
    }

    for edge in edges {
        out.extend_from_slice(&edge.src.to_le_bytes());
        out.extend_from_slice(&edge.dst.to_le_bytes());
        out.push(edge_kind_code(edge.kind.as_str()));
    }

    out.extend_from_slice(&string_table);

    fs::write(path, out)?;
    Ok(())
}

fn node_kind_code(kind: &str) -> u8 {
    match kind {
        "FUNCTION" => 1,
        "METHOD" => 2,
        "STRUCT" => 3,
        "ENUM" => 4,
        "TRAIT" => 5,
        "IMPL" => 6,
        "FIELD" => 7,
        "PARAM" => 8,
        "VARIABLE" => 9,
        "MODULE" => 10,
        "TYPE" => 11,
        "BASIC_BLOCK" => 12,
        "CALL_SITE" => 13,
        "ERROR" => 14,
        _ => 0,
    }
}

fn edge_kind_code(kind: &str) -> u8 {
    match kind {
        "CONTAINS" => 1,
        "HAS_FIELD" => 2,
        "HAS_METHOD" => 3,
        "HAS_BLOCK" => 4,
        "HAS_PARAM" => 5,
        "IMPORTS" => 6,
        "EXPORT" => 7,
        "PUBLIC_USE" => 8,
        "FLOW" => 9,
        "CALL" => 10,
        "RETURN" => 11,
        "UNWIND" => 12,
        "IMPLEMENTS" => 13,
        "FOR_TYPE" => 14,
        "USES_TYPE" => 15,
        "BOUNDS" => 16,
        "ASSIGN" => 17,
        "PROPAGATES" => 18,
        "ARG_TO_PARAM" => 19,
        "RETURNS" => 20,
        "ERROR_TO_FUNCTION" => 21,
        "ERROR_TO_BLOCK" => 22,
        _ => 0,
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_graph_bin(path: &Path) -> Result<(Vec<NodeRow>, Vec<EdgeRow>, Vec<String>)> {
    const HEADER_SIZE: usize = 32;
    const NODE_RECORD_SIZE: usize = 21;
    const EDGE_RECORD_SIZE: usize = 9;
    const NO_FILE_ID: u32 = u32::MAX;
    const NO_LINE: u32 = u32::MAX;

    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];
    if data.len() < HEADER_SIZE {
        return Err(anyhow!("graph.bin too small"));
    }
    if &data[0..4] != b"CGBN" {
        return Err(anyhow!("graph.bin magic mismatch"));
    }
    let version = u32::from_le_bytes(data[4..8].try_into()?);
    if version != 1 {
        return Err(anyhow!("graph.bin version mismatch"));
    }
    let n_nodes = u32::from_le_bytes(data[8..12].try_into()?) as usize;
    let n_edges = u32::from_le_bytes(data[12..16].try_into()?) as usize;
    let n_files = u32::from_le_bytes(data[16..20].try_into()?) as usize;
    let str_table_offset = u32::from_le_bytes(data[20..24].try_into()?) as usize;

    let nodes_offset = HEADER_SIZE;
    let edges_offset = nodes_offset + n_nodes * NODE_RECORD_SIZE;
    let expected_str_offset = edges_offset + n_edges * EDGE_RECORD_SIZE;
    if str_table_offset != expected_str_offset {
        return Err(anyhow!("graph.bin string table offset mismatch"));
    }
    if data.len() < str_table_offset {
        return Err(anyhow!("graph.bin truncated"));
    }

    let string_table = &data[str_table_offset..];

    let mut files: Vec<String> = Vec::with_capacity(n_files);
    let mut cursor = 0usize;
    for _ in 0..n_files {
        let start = cursor;
        while cursor < string_table.len() && string_table[cursor] != 0 {
            cursor += 1;
        }
        let s = std::str::from_utf8(&string_table[start..cursor]).unwrap_or("").to_string();
        files.push(s);
        cursor = cursor.saturating_add(1);
    }

    let mut nodes: Vec<NodeRow> = Vec::with_capacity(n_nodes);
    let mut pos = nodes_offset;
    for _ in 0..n_nodes {
        let id = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let kind_code = data[pos + 4];
        let file_id = u32::from_le_bytes(data[pos + 5..pos + 9].try_into()?);
        let line = u32::from_le_bytes(data[pos + 9..pos + 13].try_into()?);
        let sym_off = u32::from_le_bytes(data[pos + 13..pos + 17].try_into()?) as usize;
        let sym_len = u32::from_le_bytes(data[pos + 17..pos + 21].try_into()?) as usize;
        pos += NODE_RECORD_SIZE;

        let symbol = if sym_len == 0 {
            String::new()
        } else {
            let end = sym_off.saturating_add(sym_len);
            if end <= string_table.len() {
                std::str::from_utf8(&string_table[sym_off..end])
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        };

        nodes.push(NodeRow {
            id,
            kind: node_kind_str(kind_code).to_string(),
            symbol,
            file_id: if file_id == NO_FILE_ID { None } else { Some(file_id) },
            line: if line == NO_LINE { None } else { Some(line) },
        });
    }

    let mut edges: Vec<EdgeRow> = Vec::with_capacity(n_edges);
    let mut pos = edges_offset;
    for _ in 0..n_edges {
        let src = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let dst = u32::from_le_bytes(data[pos + 4..pos + 8].try_into()?);
        let kind_code = data[pos + 8];
        pos += EDGE_RECORD_SIZE;
        edges.push(EdgeRow {
            src,
            dst,
            kind: edge_kind_str(kind_code).to_string(),
        });
    }

    Ok((nodes, edges, files))
}

fn node_kind_str(code: u8) -> &'static str {
    match code {
        1 => "FUNCTION",
        2 => "METHOD",
        3 => "STRUCT",
        4 => "ENUM",
        5 => "TRAIT",
        6 => "IMPL",
        7 => "FIELD",
        8 => "PARAM",
        9 => "VARIABLE",
        10 => "MODULE",
        11 => "TYPE",
        12 => "BASIC_BLOCK",
        13 => "CALL_SITE",
        14 => "ERROR",
        _ => "UNKNOWN",
    }
}

fn edge_kind_str(code: u8) -> &'static str {
    match code {
        1 => "CONTAINS",
        2 => "HAS_FIELD",
        3 => "HAS_METHOD",
        4 => "HAS_BLOCK",
        5 => "HAS_PARAM",
        6 => "IMPORTS",
        7 => "EXPORT",
        8 => "PUBLIC_USE",
        9 => "FLOW",
        10 => "CALL",
        11 => "RETURN",
        12 => "UNWIND",
        13 => "IMPLEMENTS",
        14 => "FOR_TYPE",
        15 => "USES_TYPE",
        16 => "BOUNDS",
        17 => "ASSIGN",
        18 => "PROPAGATES",
        19 => "ARG_TO_PARAM",
        20 => "RETURNS",
        21 => "ERROR_TO_FUNCTION",
        22 => "ERROR_TO_BLOCK",
        _ => "UNKNOWN",
    }
}

fn snapshot_into_rows(
    snapshot: KernelSnapshot,
) -> (Vec<NodeRow>, Vec<EdgeRow>, Vec<String>) {
    let mut files = snapshot.files;
    let mut file_map: HashMap<String, u32> = HashMap::new();
    for (idx, path) in files.iter().enumerate() {
        file_map.insert(path.clone(), idx as u32);
    }

    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut key_to_id: HashMap<(String, String), u32> = HashMap::new();
    for node in snapshot.nodes {
        let file_id = if node.file.is_empty() {
            None
        } else if let Some(id) = file_map.get(&node.file).copied() {
            Some(id)
        } else {
            files.push(node.file.clone());
            let id = (files.len() - 1) as u32;
            file_map.insert(node.file.clone(), id);
            Some(id)
        };
        let id = nodes.len() as u32;
        key_to_id.insert((node.symbol.clone(), node.kind.clone()), id);
        nodes.push(NodeRow {
            id,
            kind: node.kind,
            symbol: node.symbol,
            file_id,
            line: Some(node.line),
        });
    }

    let mut edges: Vec<EdgeRow> = Vec::new();
    for edge in snapshot.edges {
        let src_id = key_to_id
            .get(&(edge.src_symbol.clone(), edge.src_kind.clone()))
            .copied()
            .unwrap_or_else(|| {
                let id = nodes.len() as u32;
                key_to_id.insert((edge.src_symbol.clone(), edge.src_kind.clone()), id);
                nodes.push(NodeRow {
                    id,
                    kind: edge.src_kind.clone(),
                    symbol: edge.src_symbol.clone(),
                    file_id: None,
                    line: None,
                });
                id
            });
        let dst_id = key_to_id
            .get(&(edge.dst_symbol.clone(), edge.dst_kind.clone()))
            .copied()
            .unwrap_or_else(|| {
                let id = nodes.len() as u32;
                key_to_id.insert((edge.dst_symbol.clone(), edge.dst_kind.clone()), id);
                nodes.push(NodeRow {
                    id,
                    kind: edge.dst_kind.clone(),
                    symbol: edge.dst_symbol.clone(),
                    file_id: None,
                    line: None,
                });
                id
            });
        edges.push(EdgeRow {
            src: src_id,
            dst: dst_id,
            kind: edge.kind,
        });
    }

    (nodes, edges, files)
}

fn build_modulegraph_from_cache(cache: &GraphCache) -> (Vec<(u32, u32)>, Vec<ModuleNode>) {
    let mut module_files = cache.module_files.clone();
    if !module_files.contains_key("") {
        module_files.insert("".to_string(), String::new());
    }

    let mut module_nodes: Vec<ModuleNode> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, file) in module_files.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        module_nodes.push(ModuleNode {
            id,
            symbol: if symbol.is_empty() { "crate".to_string() } else { symbol.clone() },
            file: file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    for symbol in module_files.keys() {
        if symbol.is_empty() {
            continue;
        }
        let parent_symbol = match symbol.rsplit_once("::") {
            Some((parent, _child)) => parent,
            None => "",
        };
        let Some(&parent_id) = symbol_to_id.get(parent_symbol) else { continue };
        let Some(&child_id) = symbol_to_id.get(symbol) else { continue };
        edges.insert((parent_id, child_id));
    }

    (edges.into_iter().collect(), module_nodes)
}

#[derive(Debug, Clone)]
struct TypeNodeRow {
    id: u32,
    symbol: String,
    file: String,
}

fn build_typegraph_from_cache(cache: &GraphCache) -> (Vec<(u32, u32, String)>, Vec<TypeNodeRow>) {
    let mut nodes: Vec<TypeNodeRow> = Vec::new();
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_id: u32 = 0;

    for (symbol, node) in cache.type_nodes.iter() {
        let id = next_id;
        next_id += 1;
        symbol_to_id.insert(symbol.clone(), id);
        nodes.push(TypeNodeRow {
            id,
            symbol: symbol.clone(),
            file: node.file.clone(),
        });
    }

    let mut edges: BTreeSet<(u32, u32, String)> = BTreeSet::new();
    for edge in cache.type_edges.iter() {
        let src_id = match symbol_to_id.get(&edge.src) {
            Some(id) => *id,
            None => {
                let id = next_id;
                next_id += 1;
                symbol_to_id.insert(edge.src.clone(), id);
                nodes.push(TypeNodeRow {
                    id,
                    symbol: edge.src.clone(),
                    file: String::new(),
                });
                id
            }
        };
        let dst_id = match symbol_to_id.get(&edge.dst) {
            Some(id) => *id,
            None => {
                let id = next_id;
                next_id += 1;
                symbol_to_id.insert(edge.dst.clone(), id);
                nodes.push(TypeNodeRow {
                    id,
                    symbol: edge.dst.clone(),
                    file: String::new(),
                });
                id
            }
        };
        edges.insert((src_id, dst_id, edge.rel.clone()));
    }

    if edges.is_empty() {
        for node in &nodes {
            edges.insert((node.id, node.id, "DECL".to_string()));
        }
    }

    (edges.into_iter().collect(), nodes)
}

fn write_typegraph_csv_from_cache(
    out_dir: &Path,
    typegraph: &[(u32, u32, String)],
    nodes: &[TypeNodeRow],
) -> Result<()> {
    let path = out_dir.join("typegraph.csv");
    let mut buf = String::with_capacity(typegraph.len() * 80 + 64);
    buf.push_str("type_a,type_b,relation,type_a_symbol,type_b_symbol,type_a_file,type_b_file\n");
    for (a, b, rel) in typegraph {
        let a_node = nodes.iter().find(|n| n.id == *a);
        let b_node = nodes.iter().find(|n| n.id == *b);
        let a_sym = a_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let b_sym = b_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let a_file = a_node.map(|n| n.file.as_str()).unwrap_or("");
        let b_file = b_node.map(|n| n.file.as_str()).unwrap_or("");
        buf.push_str(&format!("{a},{b},{rel},{a_sym},{b_sym},{a_file},{b_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

fn build_typegraph_edges(nodes: &[NodeRow], edges: &[EdgeRow]) -> Vec<(u32, u32, String)> {
    let id_to_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let type_kinds = ["STRUCT", "ENUM", "TRAIT", "IMPL", "TYPE"];
    let rel_kinds = ["HAS_FIELD", "HAS_METHOD", "IMPLEMENTS", "FOR_TYPE", "USES_TYPE", "BOUNDS"];
    let mut seen: BTreeSet<(u32, u32, String)> = BTreeSet::new();

    for edge in edges {
        if !rel_kinds.contains(&edge.kind.as_str()) {
            continue;
        }
        let src_kind = id_to_kind.get(&edge.src);
        let dst_kind = id_to_kind.get(&edge.dst);
        let src_ok = src_kind.map(|k| type_kinds.contains(k)).unwrap_or(false);
        let dst_ok = dst_kind.map(|k| type_kinds.contains(k)).unwrap_or(false);
        if src_ok && dst_ok {
            seen.insert((edge.src, edge.dst, edge.kind.clone()));
        }
    }

    if seen.is_empty() {
        for node in nodes {
            if type_kinds.contains(&node.kind.as_str()) {
                seen.insert((node.id, node.id, "DECL".to_string()));
            }
        }
    }

    seen.into_iter().collect()
}

fn write_cfg_csv(out_dir: &Path, cfg: &[EdgeRow]) -> Result<()> {
    let path = out_dir.join("cfg.csv");
    let mut buf = String::with_capacity(cfg.len() * 24 + 32);
    buf.push_str("src_block,dst_block,edge_kind\n");
    for edge in cfg {
        buf.push_str(&edge.src.to_string());
        buf.push(',');
        buf.push_str(&edge.dst.to_string());
        buf.push(',');
        buf.push_str(&edge.kind);
        buf.push('\n');
    }
    fs::write(path, buf)?;
    Ok(())
}

fn write_callgraph_csv(
    out_dir: &Path,
    callgraph: &[(u32, u32)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("callgraph.csv");
    let mut buf = String::with_capacity(callgraph.len() * 64 + 64);
    buf.push_str("caller_node,callee_node,caller_symbol,callee_symbol,caller_file,callee_file\n");
    for (caller, callee) in callgraph {
        let caller_node = nodes.iter().find(|n| n.id == *caller);
        let callee_node = nodes.iter().find(|n| n.id == *callee);
        let caller_sym = caller_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let callee_sym = callee_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let caller_file = caller_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let callee_file = callee_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        buf.push_str(&format!("{caller},{callee},{caller_sym},{callee_sym},{caller_file},{callee_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

fn write_modulegraph_csv(
    out_dir: &Path,
    modulegraph: &[(u32, u32)],
    module_nodes: &[ModuleNode],
) -> Result<()> {
    let path = out_dir.join("modulegraph.csv");
    let mut buf = String::with_capacity(modulegraph.len() * 64 + 64);
    buf.push_str("parent_module,child_module,parent_symbol,child_symbol,parent_file,child_file\n");
    for (parent, child) in modulegraph {
        let parent_node = module_nodes.iter().find(|n| n.id == *parent);
        let child_node = module_nodes.iter().find(|n| n.id == *child);
        let parent_sym = parent_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let child_sym = child_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let parent_file = parent_node.map(|n| n.file.as_str()).unwrap_or("");
        let child_file = child_node.map(|n| n.file.as_str()).unwrap_or("");
        buf.push_str(&format!("{parent},{child},{parent_sym},{child_sym},{parent_file},{child_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

fn write_typegraph_csv(
    out_dir: &Path,
    typegraph: &[(u32, u32, String)],
    nodes: &[NodeRow],
    files: &[String],
) -> Result<()> {
    let path = out_dir.join("typegraph.csv");
    let mut buf = String::with_capacity(typegraph.len() * 80 + 64);
    buf.push_str("type_a,type_b,relation,type_a_symbol,type_b_symbol,type_a_file,type_b_file\n");
    for (a, b, rel) in typegraph {
        let a_node = nodes.iter().find(|n| n.id == *a);
        let b_node = nodes.iter().find(|n| n.id == *b);
        let a_sym = a_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let b_sym = b_node.map(|n| n.symbol.as_str()).unwrap_or("");
        let a_file = a_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        let b_file = b_node
            .and_then(|n| n.file_id)
            .and_then(|id| files.get(id as usize))
            .map(|s| s.as_str())
            .unwrap_or("");
        buf.push_str(&format!("{a},{b},{rel},{a_sym},{b_sym},{a_file},{b_file}\n"));
    }
    fs::write(path, buf)?;
    Ok(())
}

fn build_branch_pressure(
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
) -> Vec<BranchPressureEntry> {
    let mut per_fn: HashMap<u32, (usize, usize)> = HashMap::new();
    for (block, outs) in cfg_out {
        let Some(owner) = block_owner.get(block).copied() else {
            continue;
        };
        let branch_nodes = if outs.len() > 1 { 1 } else { 0 };
        let pressure = outs.len().saturating_sub(1);
        let entry = per_fn.entry(owner).or_insert((0, 0));
        entry.0 += branch_nodes;
        entry.1 += pressure;
    }

    per_fn
        .into_iter()
        .filter_map(|(fn_id, (branches, pressure))| {
            let node = node_map.get(&fn_id)?;
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            Some(BranchPressureEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                branch_nodes: branches,
                branch_pressure: pressure,
            })
        })
        .collect()
}

fn build_merge_candidates(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<MergeCandidateEntry> {
    let mut out: Vec<MergeCandidateEntry> = Vec::new();
    for (block, outs) in cfg_out {
        if outs.len() < 2 {
            continue;
        }
        let mut groups: HashMap<BTreeSet<u32>, Vec<u32>> = HashMap::new();
        for succ in outs {
            let succ_outs = cfg_out.get(succ).cloned().unwrap_or_default();
            let key: BTreeSet<u32> = succ_outs.into_iter().collect();
            groups.entry(key).or_default().push(*succ);
        }
        for (key, group) in groups {
            if group.len() < 2 {
                continue;
            }
            let fn_id = block_owner.get(block).copied().unwrap_or_default();
            let fn_symbol = node_map
                .get(&fn_id)
                .map(|n| n.symbol.clone())
                .unwrap_or_default();
            let _fn_file = node_map
                .get(&fn_id)
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            out.push(MergeCandidateEntry {
                function: fn_symbol,
                branch_block: *block,
                successors: key.into_iter().collect(),
                candidate_blocks: group,
            });
        }
    }
    out
}

fn build_reachability_report(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<ReachabilityEntry> {
    let mut blocks_by_fn: HashMap<u32, Vec<u32>> = HashMap::new();
    for (block, fn_id) in block_owner {
        blocks_by_fn.entry(*fn_id).or_default().push(*block);
    }
    let mut entries = Vec::new();
    for (fn_id, blocks) in blocks_by_fn {
        let mut incoming: HashMap<u32, usize> = HashMap::new();
        for block in &blocks {
            if let Some(outs) = cfg_out.get(block) {
                for dst in outs {
                    *incoming.entry(*dst).or_insert(0) += 1;
                }
            }
        }
        let mut roots: Vec<u32> = blocks
            .iter()
            .copied()
            .filter(|b| !incoming.contains_key(b))
            .collect();
        if roots.is_empty() {
            roots.extend(blocks.iter().copied().take(1));
        }
        let mut visited: HashSet<u32> = HashSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        for root in roots {
            visited.insert(root);
            queue.push_back(root);
        }
        while let Some(current) = queue.pop_front() {
            if let Some(outs) = cfg_out.get(&current) {
                for dst in outs {
                    if visited.insert(*dst) {
                        queue.push_back(*dst);
                    }
                }
            }
        }
        let total = blocks.len();
        let reachable = visited.len();
        let ratio = if total == 0 { 0.0 } else { reachable as f64 / total as f64 };
        if let Some(node) = node_map.get(&fn_id) {
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            entries.push(ReachabilityEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                reachable_blocks: reachable,
                total_blocks: total,
                reachable_ratio: ratio,
            });
        }
    }
    entries
}

fn build_path_redundancy(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<PathRedundancyEntry> {
    let mut blocks_by_fn: HashMap<u32, Vec<u32>> = HashMap::new();
    for (block, fn_id) in block_owner {
        blocks_by_fn.entry(*fn_id).or_default().push(*block);
    }
    let mut entries = Vec::new();
    for (fn_id, blocks) in blocks_by_fn {
        let mut total = 0usize;
        let mut unique: HashSet<(u32, u32, u32)> = HashSet::new();
        for block in &blocks {
            if let Some(outs) = cfg_out.get(block) {
                for dst in outs {
                    let nexts = cfg_out.get(dst).cloned().unwrap_or_default();
                    if nexts.is_empty() {
                        total += 1;
                        unique.insert((*block, *dst, *dst));
                    } else {
                        for next in nexts {
                            total += 1;
                            unique.insert((*block, *dst, next));
                        }
                    }
                }
            }
        }
        let unique_count = unique.len();
        let ratio = if total == 0 { 0.0 } else { unique_count as f64 / total as f64 };
        if let Some(node) = node_map.get(&fn_id) {
            let file = node
                .file_id
                .and_then(|id| file_map.get(&id))
                .cloned()
                .unwrap_or_default();
            entries.push(PathRedundancyEntry {
                symbol: node.symbol.clone(),
                file,
                line: node.line,
                paths_total: total,
                paths_unique: unique_count,
                redundancy_ratio: ratio,
            });
        }
    }
    entries
}

fn write_report<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, data)?;
    Ok(())
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<NodeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(7, ',').collect();
        if parts.len() < 7 {
            continue;
        }
        let id: u32 = parts[0].parse().unwrap_or(0);
        let kind = parts[1].to_string();
        let symbol = parts[2].to_string();
        let file_id = parts[3].parse::<u32>().ok();
        let line = parts[4].parse::<u32>().ok();
        out.push(NodeRow { id, kind, symbol, file_id, line });
    }
    Ok(out)
}

fn read_edges_csv(path: PathBuf) -> Result<Vec<EdgeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        let kind = parts[2].to_string();
        out.push(EdgeRow { src, dst, kind });
    }
    Ok(out)
}

fn read_cfg_csv(path: PathBuf) -> Result<Vec<EdgeRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        let kind = parts[2].to_string();
        out.push(EdgeRow { src, dst, kind });
    }
    Ok(out)
}

fn read_callgraph_csv(path: PathBuf) -> Result<Vec<(u32, u32)>> {
    let content = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ',').collect();
        if parts.len() < 2 {
            continue;
        }
        let src: u32 = parts[0].parse().unwrap_or(0);
        let dst: u32 = parts[1].parse().unwrap_or(0);
        out.push((src, dst));
    }
    Ok(out)
}

fn read_files_txt(path: PathBuf) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let mut files = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let id = parts[0].parse::<usize>().unwrap_or(usize::MAX);
        if id == usize::MAX {
            continue;
        }
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
}

fn build_cfg_out(cfg: &[EdgeRow]) -> HashMap<u32, Vec<u32>> {
    let mut out = HashMap::new();
    for e in cfg {
        out.entry(e.src).or_insert_with(Vec::new).push(e.dst);
    }
    out
}

fn build_cfg_in(cfg: &[EdgeRow]) -> HashMap<u32, usize> {
    let mut inn = HashMap::new();
    for e in cfg {
        *inn.entry(e.dst).or_insert(0) += 1;
    }
    inn
}

fn build_block_owner(nodes: &[NodeRow], edges: &[EdgeRow]) -> HashMap<u32, u32> {
    let node_kind: HashMap<u32, &str> = nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let mut out = HashMap::new();
    for e in edges {
        if e.kind != "HAS_BLOCK" {
            continue;
        }
        let sk = node_kind.get(&e.src).copied().unwrap_or("");
        let dk = node_kind.get(&e.dst).copied().unwrap_or("");
        if (sk == "FUNCTION" || sk == "METHOD") && dk == "BASIC_BLOCK" {
            out.insert(e.dst, e.src);
        }
    }
    out
}

fn build_block_effect_signatures(edges: &[EdgeRow], node_map: &HashMap<u32, NodeRow>) -> HashMap<u32, Vec<String>> {
    let mut effects: HashMap<u32, Vec<String>> = HashMap::new();
    let ignore = ["FLOW", "UNWIND", "HAS_BLOCK"];
    for e in edges {
        if ignore.contains(&e.kind.as_str()) {
            continue;
        }
        if node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
            effects.entry(e.src).or_default().push(e.kind.clone());
        }
        if node_map.get(&e.dst).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
            effects.entry(e.dst).or_default().push(e.kind.clone());
        }
    }
    for v in effects.values_mut() {
        v.sort();
    }
    effects
}

fn trace_path(start: u32, cfg_out: &HashMap<u32, Vec<u32>>, cfg_in: &HashMap<u32, usize>) -> Vec<u32> {
    let mut path = vec![start];
    let mut current = start;
    let mut depth = 0usize;
    while depth < 50 {
        let outs = cfg_out.get(&current).map(|v| v.as_slice()).unwrap_or(&[]);
        if outs.len() != 1 {
            break;
        }
        let next = outs[0];
        if path.contains(&next) {
            break;
        }
        path.push(next);
        if *cfg_in.get(&next).unwrap_or(&0) > 1 {
            break;
        }
        current = next;
        depth += 1;
    }
    path
}

fn build_branch_complexity(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<BranchComplexityEntry> {
    let mut out = Vec::new();
    for (_block_id, entry) in build_branch_complexity_with_ids(nodes, node_map, file_map, cfg_out, cfg_in, block_effect_sig) {
        out.push(entry);
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

fn build_branch_complexity_with_ids(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<(u32, BranchComplexityEntry)> {
    let mut out = Vec::new();
    for node in nodes {
        if node.kind != "BASIC_BLOCK" {
            continue;
        }
        let outs = cfg_out.get(&node.id).map(|v| v.len()).unwrap_or(0);
        if outs < 2 {
            continue;
        }
        let branch_paths: Vec<Vec<u32>> = cfg_out
            .get(&node.id)
            .unwrap_or(&Vec::new())
            .iter()
            .map(|dst| trace_path(*dst, cfg_out, cfg_in))
            .collect();
        let mut dup_blocks = 0usize;
        let mut seq_counts: BTreeMap<Vec<u32>, usize> = BTreeMap::new();
        for p in &branch_paths {
            *seq_counts.entry(p.clone()).or_insert(0) += 1;
        }
        for (seq, count) in seq_counts.iter() {
            if *count > 1 && seq.len() > dup_blocks {
                dup_blocks = seq.len();
            }
        }
        if dup_blocks == 0 {
            let mut eff_counts: BTreeMap<Vec<Vec<String>>, usize> = BTreeMap::new();
            for p in &branch_paths {
                let eff: Vec<Vec<String>> = p.iter().map(|b| block_effect_sig.get(b).cloned().unwrap_or_default()).collect();
                *eff_counts.entry(eff).or_insert(0) += 1;
            }
            for (seq, count) in eff_counts.iter() {
                if *count > 1 && seq.len() > dup_blocks {
                    dup_blocks = seq.len();
                }
            }
        }
        let file = node.file_id.and_then(|id| file_map.get(&id).cloned()).unwrap_or_default();
        let score = outs * dup_blocks;
        let symbol = node_map.get(&node.id).map(|n| n.symbol.clone()).unwrap_or_default();
        out.push((
            node.id,
            BranchComplexityEntry {
                symbol,
                file,
                line: node.line,
                branch_count: outs,
                duplicate_block_count: dup_blocks,
                score,
            },
        ));
    }
    out.sort_by(|a, b| b.1.score.cmp(&a.1.score));
    out
}

fn build_callgraph_centrality(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<CallgraphCentralityEntry> {
    let mut callers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    let mut callees: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for (s, d) in callgraph {
        callers.entry(*d).or_default().insert(*s);
        callees.entry(*s).or_default().insert(*d);
    }
    let mut out = Vec::new();
    let mut node_ids: BTreeSet<u32> = BTreeSet::new();
    for (s, d) in callgraph {
        node_ids.insert(*s);
        node_ids.insert(*d);
    }
    for id in node_ids {
        let node = node_map.get(&id);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let caller_count = callers.get(&id).map(|s| s.len()).unwrap_or(0);
        let callee_count = callees.get(&id).map(|s| s.len()).unwrap_or(0);
        let centrality_score = caller_count + callee_count;
        out.push(CallgraphCentralityEntry { symbol, file, caller_count, callee_count, centrality_score });
    }
    out.sort_by(|a, b| b.centrality_score.cmp(&a.centrality_score));
    out
}

fn build_dead_code(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    callgraph: &[(u32, u32)],
    block_owner: &HashMap<u32, u32>,
) -> Vec<DeadCodeEntry> {
    let mut fn_nodes: HashSet<u32> = HashSet::new();
    let mut blocks: HashSet<u32> = HashSet::new();
    for n in nodes {
        if n.kind == "FUNCTION" || n.kind == "METHOD" {
            fn_nodes.insert(n.id);
        } else if n.kind == "BASIC_BLOCK" {
            blocks.insert(n.id);
        }
    }

    let mut call_adj: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut call_in: HashMap<u32, usize> = HashMap::new();
    for (s, d) in callgraph {
        if fn_nodes.contains(s) && fn_nodes.contains(d) {
            call_adj.entry(*s).or_default().push(*d);
            *call_in.entry(*d).or_insert(0) += 1;
        }
    }

    let mut entrypoints: Vec<u32> = fn_nodes.iter().copied().filter(|f| *call_in.get(f).unwrap_or(&0) == 0).collect();
    for f in &fn_nodes {
        if let Some(sym) = node_map.get(f).map(|n| n.symbol.as_str()) {
            if sym.ends_with("::main::fn") || sym == "main::fn" {
                entrypoints.push(*f);
            }
        }
    }

    let mut reachable_fns = HashSet::new();
    let mut stack: Vec<u32> = entrypoints;
    while let Some(f) = stack.pop() {
        if !reachable_fns.insert(f) {
            continue;
        }
        if let Some(next) = call_adj.get(&f) {
            for n in next {
                if !reachable_fns.contains(n) {
                    stack.push(*n);
                }
            }
        }
    }

    let mut fn_to_blocks: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in edges {
        if e.kind != "HAS_BLOCK" {
            continue;
        }
        if node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("FUNCTION")
            || node_map.get(&e.src).map(|n| n.kind.as_str()) == Some("METHOD")
        {
            if node_map.get(&e.dst).map(|n| n.kind.as_str()) == Some("BASIC_BLOCK") {
                fn_to_blocks.entry(e.src).or_default().push(e.dst);
            }
        }
    }

    let mut reachable_blocks: HashSet<u32> = HashSet::new();
    for f in &reachable_fns {
        let blocks = fn_to_blocks.get(f).cloned().unwrap_or_default();
        if blocks.is_empty() {
            continue;
        }
        let entries: Vec<u32> = blocks
            .iter()
            .copied()
            .filter(|b| cfg_in.get(b).copied().unwrap_or(0) == 0)
            .collect();
        let mut queue: VecDeque<u32> = if entries.is_empty() { VecDeque::from(vec![blocks[0]]) } else { VecDeque::from(entries) };
        let mut seen: HashSet<u32> = HashSet::new();
        while let Some(b) = queue.pop_front() {
            if !seen.insert(b) {
                continue;
            }
            reachable_blocks.insert(b);
            if let Some(outs) = cfg_out.get(&b) {
                for dst in outs {
                    if block_owner.get(dst).copied() == Some(*f) {
                        queue.push_back(*dst);
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    for f in fn_nodes {
        if !reachable_fns.contains(&f) {
            let node = node_map.get(&f);
            let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
            let file = node
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id).cloned())
                .unwrap_or_default();
            let line = node.and_then(|n| n.line);
            out.push(DeadCodeEntry { symbol, file, line, reason: "unreachable function".to_string() });
        }
    }
    for b in blocks {
        if !reachable_blocks.contains(&b) {
            let node = node_map.get(&b);
            let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
            let file = node
                .and_then(|n| n.file_id)
                .and_then(|id| file_map.get(&id).cloned())
                .unwrap_or_default();
            let line = node.and_then(|n| n.line);
            out.push(DeadCodeEntry { symbol, file, line, reason: "unreachable basic block".to_string() });
        }
    }
    out
}

fn build_callgraph_csr(callgraph: &[(u32, u32)]) -> (Csr, Vec<u32>, Vec<u32>) {
    let mut id_to_local: HashMap<u32, u32> = HashMap::new();
    let mut local_to_id: Vec<u32> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();

    for (src, dst) in callgraph {
        let src_local = *id_to_local.entry(*src).or_insert_with(|| {
            let id = local_to_id.len() as u32;
            local_to_id.push(*src);
            id
        });
        let dst_local = *id_to_local.entry(*dst).or_insert_with(|| {
            let id = local_to_id.len() as u32;
            local_to_id.push(*dst);
            id
        });
        edges.push((src_local as usize, dst_local as usize));
    }

    let csr = Csr::from_edges(local_to_id.len(), &edges);

    let mut id_to_local_vec = vec![0u32; local_to_id.len()];
    for (id, local) in id_to_local {
        id_to_local_vec[local as usize] = id;
    }

    (csr, id_to_local_vec, local_to_id)
}

fn build_dead_code_gpu(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    callgraph: &[(u32, u32)],
    block_owner: &HashMap<u32, u32>,
    cg_csr: &Csr,
    _cg_id_to_local: &[u32],
    cg_local_to_id: &[u32],
) -> Vec<DeadCodeEntry> {
    #[cfg(feature = "cuda")]
    let reachable_callgraph = {
        let roots = find_callgraph_roots(callgraph);
        let roots_local: Vec<usize> = roots
            .iter()
            .filter_map(|id| cg_local_to_id.iter().position(|x| x == id))
            .collect();
        let visited = reachability_gpu(cg_csr, &roots_local);
        visited
            .into_iter()
            .enumerate()
            .filter_map(|(idx, ok)| if ok { Some(idx as u32) } else { None })
            .collect::<Vec<u32>>()
    };

    #[cfg(not(feature = "cuda"))]
    let reachable_callgraph = {
        let roots = find_callgraph_roots(callgraph);
        let adj = build_callgraph_adj(callgraph);
        let mut reachable = HashSet::new();
        for root in roots {
            dfs_callgraph(&adj, root, &mut reachable);
        }
        reachable
            .into_iter()
            .filter_map(|id| cg_local_to_id.iter().position(|x| x == &id).map(|v| v as u32))
            .collect::<Vec<u32>>()
    };

    let reachable_callgraph_ids: HashSet<u32> = reachable_callgraph
        .into_iter()
        .filter_map(|local| cg_local_to_id.get(local as usize).copied())
        .collect();

    build_dead_code(
        nodes,
        node_map,
        file_map,
        edges,
        cfg_out,
        cfg_in,
        callgraph,
        block_owner,
    )
    .into_iter()
    .filter(|entry| {
        node_map
            .iter()
            .find(|(_, n)| n.symbol == entry.symbol)
            .map(|(id, _)| !reachable_callgraph_ids.contains(id))
            .unwrap_or(true)
    })
    .collect()
}

fn build_dependency_cycles_gpu(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cg_csr: &Csr,
    cg_local_to_id: &[u32],
) -> Vec<DependencyCycleEntry> {
    #[cfg(not(feature = "cuda"))]
    {
        return build_dependency_cycles(callgraph, node_map, file_map);
    }

    #[cfg(feature = "cuda")]
    {
        let sccs = scc_gpu(cg_csr);
        return sccs
            .into_iter()
            .enumerate()
            .filter_map(|(idx, comp)| {
                if comp.len() < 2 {
                    return None;
                }
                let mut nodes = Vec::new();
                let mut files = Vec::new();
                for local in comp {
                    let id = *cg_local_to_id.get(local as usize)?;
                    let node = node_map.get(&id)?;
                    nodes.push(node.symbol.clone());
                    if let Some(file_id) = node.file_id {
                        if let Some(path) = file_map.get(&file_id) {
                            files.push(path.clone());
                        }
                    }
                }
                Some(DependencyCycleEntry {
                    cycle_id: idx + 1,
                    nodes,
                    files,
                    cycle_length: comp.len(),
                })
            })
            .collect();
    }
}

fn build_reachability_report_gpu(
    cfg_out: &HashMap<u32, Vec<u32>>,
    block_owner: &HashMap<u32, u32>,
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    cg_csr: &Csr,
    _cg_id_to_local: &[u32],
    cg_local_to_id: &[u32],
) -> Vec<ReachabilityEntry> {
    #[cfg(feature = "cuda")]
    let reachable_callgraph = {
        let roots = find_callgraph_roots_from_edges(cg_local_to_id);
        let batches = reachability_batched_gpu(cg_csr, &roots);
        let mut reachable = Vec::new();
        for row in batches {
            for (idx, ok) in row.iter().enumerate() {
                if *ok {
                    reachable.push(idx as u32);
                }
            }
        }
        reachable
    };

    #[cfg(not(feature = "cuda"))]
    let reachable_callgraph: Vec<u32> = Vec::new();

    let reachable_callgraph_ids: HashSet<u32> = reachable_callgraph
        .into_iter()
        .filter_map(|local| cg_local_to_id.get(local as usize).copied())
        .collect();

    let base = build_reachability_report(cfg_out, block_owner, node_map, file_map);
    if reachable_callgraph_ids.is_empty() {
        return base;
    }

    base.into_iter()
        .filter(|entry| {
            node_map
                .iter()
                .find(|(_, n)| n.symbol == entry.symbol)
                .map(|(id, _)| !reachable_callgraph_ids.contains(id))
                .unwrap_or(true)
        })
        .collect()
}

fn find_callgraph_roots(callgraph: &[(u32, u32)]) -> Vec<u32> {
    let mut incoming = HashSet::new();
    let mut nodes = HashSet::new();
    for (src, dst) in callgraph {
        nodes.insert(*src);
        nodes.insert(*dst);
        incoming.insert(*dst);
    }
    nodes.into_iter().filter(|n| !incoming.contains(n)).collect()
}

fn find_callgraph_roots_from_edges(cg_local_to_id: &[u32]) -> Vec<u32> {
    cg_local_to_id.iter().copied().collect()
}

fn build_callgraph_adj(callgraph: &[(u32, u32)]) -> HashMap<u32, Vec<u32>> {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (src, dst) in callgraph {
        adj.entry(*src).or_default().push(*dst);
    }
    adj
}

fn dfs_callgraph(adj: &HashMap<u32, Vec<u32>>, start: u32, visited: &mut HashSet<u32>) {
    if !visited.insert(start) {
        return;
    }
    if let Some(nexts) = adj.get(&start) {
        for dst in nexts {
            dfs_callgraph(adj, *dst, visited);
        }
    }
}

fn tarjan_scc(callgraph: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut index = 0u32;
    let mut stack: Vec<u32> = Vec::new();
    let mut indices: HashMap<u32, u32> = HashMap::new();
    let mut lowlink: HashMap<u32, u32> = HashMap::new();
    let mut on_stack: HashSet<u32> = HashSet::new();
    let mut result: Vec<Vec<u32>> = Vec::new();

    let mut nodes: HashSet<u32> = HashSet::new();
    for (src, dst) in callgraph {
        nodes.insert(*src);
        nodes.insert(*dst);
    }

    fn strongconnect(
        v: u32,
        index: &mut u32,
        stack: &mut Vec<u32>,
        indices: &mut HashMap<u32, u32>,
        lowlink: &mut HashMap<u32, u32>,
        on_stack: &mut HashSet<u32>,
        result: &mut Vec<Vec<u32>>,
        callgraph: &[(u32, u32)],
    ) {
        indices.insert(v, *index);
        lowlink.insert(v, *index);
        *index += 1;
        stack.push(v);
        on_stack.insert(v);

        for (src, dst) in callgraph {
            if *src != v {
                continue;
            }
            if !indices.contains_key(dst) {
                strongconnect(*dst, index, stack, indices, lowlink, on_stack, result, callgraph);
                let low_v = *lowlink.get(&v).unwrap();
                let low_dst = *lowlink.get(dst).unwrap();
                lowlink.insert(v, low_v.min(low_dst));
            } else if on_stack.contains(dst) {
                let low_v = *lowlink.get(&v).unwrap();
                let idx_dst = *indices.get(dst).unwrap();
                lowlink.insert(v, low_v.min(idx_dst));
            }
        }

        if indices.get(&v) == lowlink.get(&v) {
            let mut scc = Vec::new();
            loop {
                if let Some(w) = stack.pop() {
                    on_stack.remove(&w);
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
            }
            result.push(scc);
        }
    }

    for node in nodes {
        if !indices.contains_key(&node) {
            strongconnect(
                node,
                &mut index,
                &mut stack,
                &mut indices,
                &mut lowlink,
                &mut on_stack,
                &mut result,
                callgraph,
            );
        }
    }
    result
}

fn build_dependency_cycles(
    callgraph: &[(u32, u32)],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
) -> Vec<DependencyCycleEntry> {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut nodes: HashSet<u32> = HashSet::new();
    for (s, d) in callgraph {
        adj.entry(*s).or_default().push(*d);
        nodes.insert(*s);
        nodes.insert(*d);
    }

    // Tarjan SCC
    let mut index = 0usize;
    let mut stack: Vec<u32> = Vec::new();
    let mut onstack: HashSet<u32> = HashSet::new();
    let mut indices: HashMap<u32, usize> = HashMap::new();
    let mut lowlink: HashMap<u32, usize> = HashMap::new();
    let mut sccs: Vec<Vec<u32>> = Vec::new();

    fn strongconnect(
        v: u32,
        index: &mut usize,
        stack: &mut Vec<u32>,
        onstack: &mut HashSet<u32>,
        indices: &mut HashMap<u32, usize>,
        lowlink: &mut HashMap<u32, usize>,
        adj: &HashMap<u32, Vec<u32>>,
        sccs: &mut Vec<Vec<u32>>,
    ) {
        indices.insert(v, *index);
        lowlink.insert(v, *index);
        *index += 1;
        stack.push(v);
        onstack.insert(v);

        if let Some(neigh) = adj.get(&v) {
            for w in neigh {
                if !indices.contains_key(w) {
                    strongconnect(*w, index, stack, onstack, indices, lowlink, adj, sccs);
                    let lw = *lowlink.get(w).unwrap_or(&0);
                    let lv = *lowlink.get(&v).unwrap_or(&0);
                    lowlink.insert(v, lv.min(lw));
                } else if onstack.contains(w) {
                    let iw = *indices.get(w).unwrap_or(&0);
                    let lv = *lowlink.get(&v).unwrap_or(&0);
                    lowlink.insert(v, lv.min(iw));
                }
            }
        }

        if lowlink.get(&v) == indices.get(&v) {
            let mut scc = Vec::new();
            loop {
                if let Some(w) = stack.pop() {
                    onstack.remove(&w);
                    scc.push(w);
                    if w == v {
                        break;
                    }
                }
            }
            sccs.push(scc);
        }
    }

    for v in nodes {
        if !indices.contains_key(&v) {
            strongconnect(v, &mut index, &mut stack, &mut onstack, &mut indices, &mut lowlink, &adj, &mut sccs);
        }
    }

    let mut out = Vec::new();
    let mut cycle_id = 0usize;
    for scc in sccs {
        let mut is_cycle = scc.len() > 1;
        if !is_cycle {
            // self-loop
            let v = scc[0];
            if let Some(neigh) = adj.get(&v) {
                if neigh.contains(&v) {
                    is_cycle = true;
                }
            }
        }
        if !is_cycle {
            continue;
        }
        let mut node_syms: Vec<String> = Vec::new();
        let mut file_set: BTreeSet<String> = BTreeSet::new();
        for n in &scc {
            if let Some(node) = node_map.get(n) {
                node_syms.push(node.symbol.clone());
                if let Some(fid) = node.file_id {
                    if let Some(path) = file_map.get(&fid) {
                        file_set.insert(path.clone());
                    }
                }
            }
        }
        node_syms.sort();
        cycle_id += 1;
        out.push(DependencyCycleEntry {
            cycle_id,
            nodes: node_syms,
            files: file_set.into_iter().collect(),
            cycle_length: scc.len(),
        });
    }
    out
}

fn build_structural_hotspots(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    callgraph: &[(u32, u32)],
    cfg_out: &HashMap<u32, Vec<u32>>,
    cfg_in: &HashMap<u32, usize>,
    block_owner: &HashMap<u32, u32>,
    block_effect_sig: &HashMap<u32, Vec<String>>,
) -> Vec<StructuralHotspotEntry> {
    let mut callers: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for (s, d) in callgraph {
        callers.entry(*d).or_default().insert(*s);
    }

    let mut branch_entries = build_branch_complexity_with_ids(nodes, node_map, file_map, cfg_out, cfg_in, block_effect_sig);
    let mut per_fn: HashMap<u32, (usize, usize)> = HashMap::new(); // fn -> (branch_count, dup_blocks)
    for (block_id, entry) in branch_entries.drain(..) {
        if let Some(fid) = block_owner.get(&block_id) {
            let e = per_fn.entry(*fid).or_insert((0, 0));
            e.0 += entry.branch_count;
            e.1 += entry.duplicate_block_count;
        }
    }

    let mut out = Vec::new();
    for (fid, (branch_count, dup_blocks)) in per_fn {
        let node = node_map.get(&fid);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        let caller_syms: Vec<String> = callers
            .get(&fid)
            .map(|s| s.iter().filter_map(|id| node_map.get(id).map(|n| n.symbol.clone())).collect())
            .unwrap_or_else(Vec::new);
        let score = branch_count * dup_blocks.max(1) * caller_syms.len().max(1);
        out.push(StructuralHotspotEntry {
            symbol,
            file,
            line,
            branch_count,
            duplicate_blocks: dup_blocks,
            callers: caller_syms,
            score,
        });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    out
}

fn build_dataflow_fanout(
    nodes: &[NodeRow],
    node_map: &HashMap<u32, NodeRow>,
    file_map: &HashMap<u32, String>,
    edges: &[EdgeRow],
    block_owner: &HashMap<u32, u32>,
) -> Vec<DataflowFanoutEntry> {
    let mut out = Vec::new();
    let mut fn_nodes: Vec<u32> = Vec::new();
    for n in nodes {
        if n.kind == "FUNCTION" || n.kind == "METHOD" {
            fn_nodes.push(n.id);
        }
    }

    let mutation_kinds: HashSet<&str> = ["ASSIGN", "PROPAGATES", "ARG_TO_PARAM", "RETURNS"].into_iter().collect();
    let io_kinds: HashSet<&str> = ["CALL", "RETURN"].into_iter().collect();

    let mut edges_by_fn: HashMap<u32, Vec<&EdgeRow>> = HashMap::new();
    for e in edges {
        let owner = block_owner.get(&e.src).copied().or_else(|| block_owner.get(&e.dst).copied());
        if let Some(fid) = owner {
            edges_by_fn.entry(fid).or_default().push(e);
        }
    }

    for fid in fn_nodes {
        let node = node_map.get(&fid);
        let symbol = node.map(|n| n.symbol.clone()).unwrap_or_default();
        let file = node
            .and_then(|n| n.file_id)
            .and_then(|id| file_map.get(&id).cloned())
            .unwrap_or_default();
        let line = node.and_then(|n| n.line);
        let fn_edges = edges_by_fn.get(&fid).cloned().unwrap_or_default();
        let outgoing_edges = fn_edges.len();
        let mutation_edges = fn_edges.iter().filter(|e| mutation_kinds.contains(e.kind.as_str())).count();
        let io_edges = fn_edges.iter().filter(|e| io_kinds.contains(e.kind.as_str())).count();
        out.push(DataflowFanoutEntry { symbol, file, line, outgoing_edges, mutation_edges, io_edges });
    }
    out.sort_by(|a, b| b.outgoing_edges.cmp(&a.outgoing_edges));
    out
}
