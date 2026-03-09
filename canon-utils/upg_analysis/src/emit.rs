use crate::types::{Edge, Node, SpanRange};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub output_dir: PathBuf,
}

pub fn write_outputs(graph: &crate::extract::UpgGraph, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    write_nodes_csv(output_dir, &graph.nodes)?;
    write_edges_csv(output_dir, &graph.edges)?;
    write_files_txt(output_dir, &graph.nodes)?;
    write_spans_bin(output_dir, &graph.nodes, &graph.spans_primary)?;
    prune_legacy_outputs(output_dir);
    write_kinds(output_dir)?;
    write_bin_u32(output_dir.join("csr_row_ptr.bin"), &graph.csr.row_ptr)?;
    write_bin_u32(output_dir.join("csr_col_idx.bin"), &graph.csr.col_idx)?;
    let metadata_path = output_dir.join("metadata.json");
    let file = fs::File::create(metadata_path)?;
    serde_json::to_writer_pretty(file, &graph.metadata)
        .map_err(|err| anyhow!("failed to write metadata.json: {err}"))?;
    let report = verify_outputs(output_dir)?;
    write_invariants(output_dir, &report)?;
    if !report.ok {
        return Err(anyhow!("analysis invariants failed: {}", report.summary()));
    }
    Ok(())
}

fn prune_legacy_outputs(output_dir: &Path) {
    for name in [
        "files.csv",
        "spans_primary.bin",
        "spans_extra.bin",
        "spans_extra.idx",
    ] {
        let path = output_dir.join(name);
        let _ = fs::remove_file(path);
    }
}

fn write_nodes_csv(output_dir: &Path, nodes: &[Node]) -> Result<()> {
    let path = output_dir.join("nodes.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "node_id,node_kind,symbol,file_id,line,column,parent")?;
    let file_ids = collect_file_ids(output_dir, nodes);
    let symbol_to_id = collect_symbol_ids(nodes);
    let node_file_ids = compute_node_file_ids(output_dir, nodes, &symbol_to_id, &file_ids);
    for node in nodes {
        let file_id = node_file_ids
            .get(node.id as usize)
            .copied()
            .unwrap_or(u32::MAX);
        let parent = compute_parent_id(node, &symbol_to_id);
        let symbol = sanitize_csv_field(&node.symbol);
        writeln!(
            file,
            "{},{},{},{},{},{},{}",
            node.id,
            node_kind_str(node.kind),
            symbol,
            file_id,
            node.line,
            node.column,
            parent
        )?;
    }
    Ok(())
}

fn write_edges_csv(output_dir: &Path, edges: &[Edge]) -> Result<()> {
    let path = output_dir.join("edges.csv");
    let mut file = fs::File::create(path)?;
    writeln!(file, "src_id,dst_id,edge_kind")?;
    for edge in edges {
        writeln!(
            file,
            "{},{},{}",
            edge.src,
            edge.dst,
            edge_kind_str(edge.kind)
        )?;
    }
    Ok(())
}

fn write_files_txt(output_dir: &Path, nodes: &[Node]) -> Result<()> {
    let path = output_dir.join("files.txt");
    let mut file = fs::File::create(path)?;
    writeln!(file, "file_id,path")?;
    let files = collect_files(output_dir, nodes);
    for (id, path_str) in files.iter().enumerate() {
        let field = sanitize_csv_field(path_str);
        writeln!(file, "{},{}", id, field)?;
    }
    Ok(())
}

fn write_spans_bin(output_dir: &Path, nodes: &[Node], spans: &[SpanRange]) -> Result<()> {
    let path = output_dir.join("spans.bin");
    let mut file = fs::File::create(path)?;
    let file_ids = collect_file_ids(output_dir, nodes);
    let symbol_to_id = collect_symbol_ids(nodes);
    let node_file_ids = compute_node_file_ids(output_dir, nodes, &symbol_to_id, &file_ids);
    for (idx, node) in nodes.iter().enumerate() {
        let span = spans.get(idx).copied().unwrap_or(SpanRange { lo: 0, hi: 0 });
        let file_id = node_file_ids
            .get(node.id as usize)
            .copied()
            .unwrap_or(u32::MAX);
        file.write_all(&node.id.to_le_bytes())?;
        file.write_all(&file_id.to_le_bytes())?;
        file.write_all(&span.lo.to_le_bytes())?;
        file.write_all(&span.hi.to_le_bytes())?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct InvariantReport {
    ok: bool,
    node_id_contiguous: bool,
    node_count: usize,
    spans_count: usize,
    spans_match_nodes: bool,
    span_ids_match_nodes: bool,
    edges_with_missing_src: usize,
    edges_with_missing_dst: usize,
    invalid_node_kinds: usize,
    invalid_edge_kinds: usize,
    bad_file_id_nodes: usize,
    bb_without_has_block: usize,
    call_without_has_block: usize,
    isolated_nodes: usize,
    module_count: usize,
    module_root_like: usize,
}

impl InvariantReport {
    fn summary(&self) -> String {
        format!(
            "nodes={}/spans={} contiguous={} missing_edges(src={}, dst={}) invalid_kinds(node={}, edge={}) bad_file_id={} bb_no_block={} call_no_block={} isolated={} module_root_like={}",
            self.node_count,
            self.spans_count,
            self.node_id_contiguous,
            self.edges_with_missing_src,
            self.edges_with_missing_dst,
            self.invalid_node_kinds,
            self.invalid_edge_kinds,
            self.bad_file_id_nodes,
            self.bb_without_has_block,
            self.call_without_has_block,
            self.isolated_nodes,
            self.module_root_like
        )
    }
}

fn write_invariants(output_dir: &Path, report: &InvariantReport) -> Result<()> {
    let path = output_dir.join("upg_invariants.json");
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)
        .map_err(|err| anyhow!("failed to write upg_invariants.json: {err}"))
}

fn verify_outputs(output_dir: &Path) -> Result<InvariantReport> {
    let nodes_path = output_dir.join("nodes.csv");
    let edges_path = output_dir.join("edges.csv");
    let files_path = output_dir.join("files.txt");
    let spans_path = output_dir.join("spans.bin");
    let node_kinds_path = output_dir.join("node_kinds.txt");
    let edge_kinds_path = output_dir.join("edge_kinds.txt");

    let node_kinds: std::collections::HashSet<String> =
        fs::read_to_string(node_kinds_path)?.lines().map(|s| s.to_string()).collect();
    let edge_kinds: std::collections::HashSet<String> =
        fs::read_to_string(edge_kinds_path)?.lines().map(|s| s.to_string()).collect();
    let files = read_files_txt(files_path)?;

    let nodes = read_nodes_csv_with_file_id(&nodes_path)?;
    let node_count = nodes.len();
    let max_id = nodes.iter().map(|n| n.id).max().unwrap_or(0);
    let mut ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut invalid_node_kinds = 0usize;
    let mut bad_file_id_nodes = 0usize;
    let mut module_count = 0usize;
    let mut module_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for node in &nodes {
        ids.insert(node.id);
        if !node_kinds.contains(node.kind_str()) {
            invalid_node_kinds += 1;
        }
        if node.file_id as usize >= files.len() || files[node.file_id as usize].is_empty() {
            bad_file_id_nodes += 1;
        }
        if node.kind == crate::types::NodeKind::Module {
            module_count += 1;
            module_ids.insert(node.id);
        }
    }
    let node_id_contiguous = ids.len() == (max_id as usize + 1);

    let edges = read_edges_csv_with_kind(&edges_path)?;
    let mut edges_with_missing_src = 0usize;
    let mut edges_with_missing_dst = 0usize;
    let mut invalid_edge_kinds = 0usize;
    let mut edge_src: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut edge_dst: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut has_block_in: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut imports_dst: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for edge in &edges {
        if !ids.contains(&edge.src) {
            edges_with_missing_src += 1;
        }
        if !ids.contains(&edge.dst) {
            edges_with_missing_dst += 1;
        }
        if !edge_kinds.contains(&edge.kind) {
            invalid_edge_kinds += 1;
        }
        edge_src.insert(edge.src);
        edge_dst.insert(edge.dst);
        if edge.kind == "HAS_BLOCK" {
            has_block_in.insert(edge.dst);
        }
        if edge.kind == "IMPORTS" {
            imports_dst.insert(edge.dst);
        }
    }

    let mut bb_without_has_block = 0usize;
    let mut call_without_has_block = 0usize;
    let mut isolated_nodes = 0usize;
    for node in &nodes {
        if node.kind == crate::types::NodeKind::BasicBlock && !has_block_in.contains(&node.id) {
            bb_without_has_block += 1;
        }
        if node.kind == crate::types::NodeKind::CallSite && !has_block_in.contains(&node.id) {
            call_without_has_block += 1;
        }
        if !edge_src.contains(&node.id) && !edge_dst.contains(&node.id) {
            isolated_nodes += 1;
        }
    }

    let module_root_like = module_ids
        .iter()
        .filter(|id| !imports_dst.contains(id))
        .count();

    let span_bytes = fs::read(spans_path)?;
    if span_bytes.len() % 16 != 0 {
        return Err(anyhow!("spans.bin length {} not divisible by 16", span_bytes.len()));
    }
    let spans_count = span_bytes.len() / 16;
    let spans_match_nodes = spans_count == node_count;
    let mut span_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for chunk in span_bytes.chunks_exact(16) {
        let node_id = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        span_ids.insert(node_id);
    }
    let span_ids_match_nodes = span_ids.len() == node_count && span_ids.iter().max().copied() == Some(max_id);

    let ok = node_id_contiguous
        && spans_match_nodes
        && span_ids_match_nodes
        && edges_with_missing_src == 0
        && edges_with_missing_dst == 0
        && invalid_node_kinds == 0
        && invalid_edge_kinds == 0
        && bad_file_id_nodes == 0
        && bb_without_has_block == 0
        && call_without_has_block == 0
        && isolated_nodes == 0
        && module_root_like == 1;

    Ok(InvariantReport {
        ok,
        node_id_contiguous,
        node_count,
        spans_count,
        spans_match_nodes,
        span_ids_match_nodes,
        edges_with_missing_src,
        edges_with_missing_dst,
        invalid_node_kinds,
        invalid_edge_kinds,
        bad_file_id_nodes,
        bb_without_has_block,
        call_without_has_block,
        isolated_nodes,
        module_count,
        module_root_like,
    })
}

#[derive(Debug)]
struct NodeRow {
    id: u32,
    kind: crate::types::NodeKind,
    file_id: u32,
}

impl NodeRow {
    fn kind_str(&self) -> &'static str {
        node_kind_str(self.kind)
    }
}

fn read_nodes_csv_with_file_id(path: &Path) -> Result<Vec<NodeRow>> {
    let content = fs::read_to_string(path)?;
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            continue;
        }
        let id = match parts[0].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = match parse_node_kind(parts[1]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let file_id = match parts[3].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        nodes.push(NodeRow { id, kind, file_id });
    }
    Ok(nodes)
}

#[derive(Debug)]
struct EdgeRow {
    src: u32,
    dst: u32,
    kind: String,
}

fn read_edges_csv_with_kind(path: &Path) -> Result<Vec<EdgeRow>> {
    let content = fs::read_to_string(path)?;
    let mut edges = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            continue;
        }
        let src = match parts[0].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let dst = match parts[1].parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        edges.push(EdgeRow { src, dst, kind: parts[2].to_string() });
    }
    Ok(edges)
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
        let id = parts[0].parse::<usize>()?;
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
}

fn parse_node_kind(raw: &str) -> Result<crate::types::NodeKind> {
    match raw {
        "FUNCTION" => Ok(crate::types::NodeKind::Function),
        "METHOD" => Ok(crate::types::NodeKind::Method),
        "STRUCT" => Ok(crate::types::NodeKind::Struct),
        "ENUM" => Ok(crate::types::NodeKind::Enum),
        "TRAIT" => Ok(crate::types::NodeKind::Trait),
        "IMPL" => Ok(crate::types::NodeKind::Impl),
        "FIELD" => Ok(crate::types::NodeKind::Field),
        "PARAM" => Ok(crate::types::NodeKind::Param),
        "VARIABLE" => Ok(crate::types::NodeKind::Variable),
        "MODULE" => Ok(crate::types::NodeKind::Module),
        "TYPE" => Ok(crate::types::NodeKind::Type),
        "BASIC_BLOCK" => Ok(crate::types::NodeKind::BasicBlock),
        "CALL_SITE" => Ok(crate::types::NodeKind::CallSite),
        "ERROR" => Ok(crate::types::NodeKind::Error),
        _ => Err(anyhow!("unknown node kind")),
    }
}

fn collect_files(output_dir: &Path, nodes: &[Node]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for node in nodes {
        if let Some(path) = normalize_file_path(output_dir, &node.file) {
            set.insert(path);
        }
    }
    set.into_iter().collect()
}

fn collect_file_ids(output_dir: &Path, nodes: &[Node]) -> BTreeMap<String, u32> {
    let files = collect_files(output_dir, nodes);
    let mut out = BTreeMap::new();
    for (idx, path) in files.into_iter().enumerate() {
        out.insert(path, idx as u32);
    }
    out
}

fn normalize_file_path(output_dir: &Path, raw: &str) -> Option<String> {
    let project_root = output_dir.parent()?;
    let mut cleaned = raw.trim().to_string();
    if cleaned.is_empty() || cleaned == "." {
        return None;
    }
    if cleaned.starts_with('"') && cleaned.ends_with('"') && cleaned.len() >= 2 {
        cleaned = cleaned[1..cleaned.len() - 1].to_string();
    }
    let mut path = PathBuf::from(&cleaned);
    if !path.is_absolute() {
        let candidate = project_root.join(&path);
        if candidate.exists() {
            path = candidate;
        } else {
            for ancestor in project_root.ancestors().skip(1).take(4) {
                let candidate = ancestor.join(&path);
                if candidate.exists() {
                    path = candidate;
                    break;
                }
            }
        }
    }
    if !path.exists() {
        return None;
    }
    if path.is_dir() {
        return None;
    }
    if !path.starts_with(project_root) {
        return None;
    }
    Some(path.to_string_lossy().to_string())
}

fn collect_symbol_ids(nodes: &[Node]) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for node in nodes {
        out.insert(node.symbol.clone(), node.id);
    }
    out
}

fn compute_node_file_ids(
    output_dir: &Path,
    nodes: &[Node],
    symbol_to_id: &BTreeMap<String, u32>,
    file_ids: &BTreeMap<String, u32>,
) -> Vec<u32> {
    let mut node_file_ids: Vec<u32> = vec![u32::MAX; nodes.len()];
    for node in nodes {
        if let Some(key) = normalize_file_path(output_dir, &node.file) {
            if let Some(file_id) = file_ids.get(&key).copied() {
                let idx = node.id as usize;
                if idx < node_file_ids.len() {
                    node_file_ids[idx] = file_id;
                }
            }
        }
    }
    // Inherit file_id from parent when unresolved (synthetic/macro nodes).
    let mut changed = true;
    let mut passes = 0usize;
    while changed && passes < nodes.len() {
        changed = false;
        passes += 1;
        for node in nodes {
            let idx = node.id as usize;
            if idx >= node_file_ids.len() || node_file_ids[idx] != u32::MAX {
                continue;
            }
            if let Some(parent_id) = compute_parent_id_opt(node, symbol_to_id) {
                let pidx = parent_id as usize;
                if pidx < node_file_ids.len() && node_file_ids[pidx] != u32::MAX {
                    node_file_ids[idx] = node_file_ids[pidx];
                    changed = true;
                }
            }
        }
    }
    node_file_ids
}

fn compute_parent_id(node: &Node, symbol_to_id: &BTreeMap<String, u32>) -> u32 {
    compute_parent_id_opt(node, symbol_to_id).unwrap_or(0)
}

fn compute_parent_id_opt(node: &Node, symbol_to_id: &BTreeMap<String, u32>) -> Option<u32> {
    let parent_symbol = match node.kind {
        crate::types::NodeKind::Module => {
            if node.symbol.is_empty() {
                None
            } else if let Some(parent) = node.symbol.rsplitn(2, "::").nth(1) {
                Some(parent.to_string())
            } else {
                Some(String::new())
            }
        }
        crate::types::NodeKind::Function | crate::types::NodeKind::Method => {
            let base = node.symbol.strip_suffix("::fn").unwrap_or(node.symbol.as_str());
            if let Some(parent) = base.rsplitn(2, "::").nth(1) {
                Some(parent.to_string())
            } else {
                Some(String::new())
            }
        }
        crate::types::NodeKind::Param => {
            node.symbol.rsplitn(2, "::").nth(1).map(str::to_string)
        }
        crate::types::NodeKind::BasicBlock => {
            if let Some(base) = node.symbol.split("::bb").next() {
                Some(format!("{base}::fn"))
            } else {
                None
            }
        }
        crate::types::NodeKind::CallSite => {
            node.symbol.rsplitn(2, "::").nth(1).map(str::to_string)
        }
        crate::types::NodeKind::Field
        | crate::types::NodeKind::Struct
        | crate::types::NodeKind::Enum
        | crate::types::NodeKind::Trait
        | crate::types::NodeKind::Impl
        | crate::types::NodeKind::Type
        | crate::types::NodeKind::Variable => {
            node.symbol.rsplitn(2, "::").nth(1).map(str::to_string)
        }
        _ => None,
    };

    parent_symbol.and_then(|sym| symbol_to_id.get(&sym).copied())
}

fn node_kind_str(kind: crate::types::NodeKind) -> &'static str {
    match kind {
        crate::types::NodeKind::Function => "FUNCTION",
        crate::types::NodeKind::Method => "METHOD",
        crate::types::NodeKind::Struct => "STRUCT",
        crate::types::NodeKind::Enum => "ENUM",
        crate::types::NodeKind::Trait => "TRAIT",
        crate::types::NodeKind::Impl => "IMPL",
        crate::types::NodeKind::Field => "FIELD",
        crate::types::NodeKind::Param => "PARAM",
        crate::types::NodeKind::Variable => "VARIABLE",
        crate::types::NodeKind::Module => "MODULE",
        crate::types::NodeKind::Type => "TYPE",
        crate::types::NodeKind::BasicBlock => "BASIC_BLOCK",
        crate::types::NodeKind::CallSite => "CALL_SITE",
        crate::types::NodeKind::Error => "ERROR",
    }
}

fn edge_kind_str(kind: crate::types::EdgeKind) -> &'static str {
    match kind {
        crate::types::EdgeKind::Contains => "CONTAINS",
        crate::types::EdgeKind::HasField => "HAS_FIELD",
        crate::types::EdgeKind::HasMethod => "HAS_METHOD",
        crate::types::EdgeKind::HasBlock => "HAS_BLOCK",
        crate::types::EdgeKind::HasParam => "HAS_PARAM",
        crate::types::EdgeKind::Imports => "IMPORTS",
        crate::types::EdgeKind::Export => "EXPORT",
        crate::types::EdgeKind::PublicUse => "PUBLIC_USE",
        crate::types::EdgeKind::Flow => "FLOW",
        crate::types::EdgeKind::Call => "CALL",
        crate::types::EdgeKind::Return => "RETURN",
        crate::types::EdgeKind::Unwind => "UNWIND",
        crate::types::EdgeKind::Implements => "IMPLEMENTS",
        crate::types::EdgeKind::ForType => "FOR_TYPE",
        crate::types::EdgeKind::UsesType => "USES_TYPE",
        crate::types::EdgeKind::Bounds => "BOUNDS",
        crate::types::EdgeKind::Assign => "ASSIGN",
        crate::types::EdgeKind::Propagates => "PROPAGATES",
        crate::types::EdgeKind::ArgToParam => "ARG_TO_PARAM",
        crate::types::EdgeKind::Returns => "RETURNS",
        crate::types::EdgeKind::ErrorToFunction => "ERROR_TO_FUNCTION",
        crate::types::EdgeKind::ErrorToBlock => "ERROR_TO_BLOCK",
    }
}

fn sanitize_csv_field(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let clean = if ch.is_control()
            || ch == '\u{2028}'
            || ch == '\u{2029}'
            || ch == '\u{0085}' {
            ' '
        } else if ch == ',' {
            ';'
        } else {
            ch
        };
        out.push(clean);
    }
    out
}

fn write_kinds(output_dir: &Path) -> Result<()> {
    let node_kinds = [
        "FUNCTION",
        "METHOD",
        "STRUCT",
        "ENUM",
        "TRAIT",
        "IMPL",
        "FIELD",
        "PARAM",
        "VARIABLE",
        "MODULE",
        "TYPE",
        "BASIC_BLOCK",
        "CALL_SITE",
        "ERROR",
    ];
    let edge_kinds = [
        "CONTAINS",
        "HAS_FIELD",
        "HAS_METHOD",
        "HAS_BLOCK",
        "HAS_PARAM",
        "IMPORTS",
        "EXPORT",
        "PUBLIC_USE",
        "FLOW",
        "CALL",
        "RETURN",
        "UNWIND",
        "IMPLEMENTS",
        "FOR_TYPE",
        "USES_TYPE",
        "BOUNDS",
        "ASSIGN",
        "PROPAGATES",
        "ARG_TO_PARAM",
        "RETURNS",
        "ERROR_TO_FUNCTION",
        "ERROR_TO_BLOCK",
    ];
    fs::write(output_dir.join("node_kinds.txt"), node_kinds.join("\n"))?;
    fs::write(output_dir.join("edge_kinds.txt"), edge_kinds.join("\n"))?;
    Ok(())
}

fn write_bin_u32(path: PathBuf, values: &[u32]) -> Result<()> {
    let mut file = fs::File::create(path)?;
    for &value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}
