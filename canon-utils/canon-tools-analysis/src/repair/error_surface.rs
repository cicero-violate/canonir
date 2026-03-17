use canon_types::{parse_edge_kind, parse_node_kind, Edge, EdgeKind, Node, NodeKind};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ErrorPayload {
    errors: Vec<Value>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairSurfaceEntry {
    pub node_id: u32,
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub error_count: usize,
}

pub fn augment_with_errors(output_dir: &Path, errors_json: &Path, out_dir: &Path) -> Result<()> {
    if !errors_json.exists() {
        return Ok(());
    }
    let payload = fs::read_to_string(errors_json)?;
    let parsed: ErrorPayload = serde_json::from_str(&payload).unwrap_or(ErrorPayload { errors: Vec::new() });
    if parsed.errors.is_empty() {
        return Ok(());
    }

    let mut nodes = read_nodes_csv(output_dir.join("nodes.csv"))?;
    let mut edges = read_edges_csv(output_dir.join("edges.csv"))?;

    let mut next_id = nodes.iter().map(|n| n.id).max().unwrap_or(0).saturating_add(1);
    let mut added_errors: Vec<(u32, ErrorSpan)> = Vec::new();
    let mut synthetic_modules: BTreeMap<String, u32> = BTreeMap::new();

    for (idx, err) in parsed.errors.iter().enumerate() {
        let span = primary_span(err);
        let (file, line, column) = span
            .as_ref()
            .map(|s| (s.file.clone(), s.line_start, s.col_start))
            .unwrap_or_else(|| ("unknown".to_string(), 0, 0));
        let code = err.get("code").and_then(|c| c.get("code")).and_then(|v| v.as_str()).unwrap_or("unknown");
        let symbol = format!("error::{code}::{idx}");
        let node_id = next_id;
        next_id = next_id.saturating_add(1);
        nodes.push(Node {
            id: node_id,
            kind: NodeKind::Error,
            symbol,
            file,
            line,
            column,
            file_id: None,
            parent: None,
        });
        if let Some(span) = span {
            added_errors.push((node_id, span));
        }
    }

    let mut edges_added = Vec::new();
    for (err_id, span) in &added_errors {
        let mut function_targets = find_nodes_at_span(&nodes, span, NodeKind::Function);
        function_targets.extend(find_nodes_at_span(&nodes, span, NodeKind::Method));

        let selected_fn = if function_targets.is_empty() {
            let file_key = normalize_file(&span.file);
            let nearest = find_nearest_node(&nodes, span, &[NodeKind::Function, NodeKind::Method])
                .or_else(|| find_nearest_node(&nodes, span, &[NodeKind::Module]))
                .map(|n| n.id);
            if nearest.is_some() {
                nearest
            } else {
                let module_id = *synthetic_modules.entry(file_key.clone()).or_insert_with(|| {
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    nodes.push(Node {
                        id,
                        kind: NodeKind::Module,
                        symbol: format!("file::{}", file_key),
                        file: file_key.clone(),
                        line: 0,
                        column: 0,
                        file_id: None,
                        parent: None,
                    });
                    id
                });
                Some(module_id)
            }
        } else {
            function_targets.iter().min_by_key(|n| &n.symbol).map(|n| n.id)
        };

        let block_targets = find_nodes_at_span(&nodes, span, NodeKind::BasicBlock);
        if let Some(dst) = selected_fn {
            edges_added.push(Edge { src: *err_id, dst, kind: EdgeKind::ErrorToFunction });
        }
        if block_targets.is_empty() {
            if let Some(block) = find_nearest_node(&nodes, span, &[NodeKind::BasicBlock]) {
                edges_added.push(Edge { src: *err_id, dst: block.id, kind: EdgeKind::ErrorToBlock });
            }
        } else {
            for block in block_targets {
                edges_added.push(Edge { src: *err_id, dst: block.id, kind: EdgeKind::ErrorToBlock });
            }
        }
    }
    edges.extend(edges_added);

    let graph = Graph { nodes, edges };

    let surface = build_repair_surface(&graph);
    write_repair_surface(out_dir, &surface)?;

    Ok(())
}

pub fn write_repair_surface(out_dir: &Path, surface: &[RepairSurfaceEntry]) -> Result<()> {
    let path = out_dir.join("repair_surface.json");
    let payload = serde_json::json!({
        "top_k": surface,
        "count": surface.len(),
    });
    fs::write(path, serde_json::to_string_pretty(&payload)?)?;
    Ok(())
}

struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

fn build_repair_surface(graph: &Graph) -> Vec<RepairSurfaceEntry> {
    let mut error_to_fn: BTreeMap<u32, u32> = BTreeMap::new();
    for e in &graph.edges {
        if matches!(e.kind, EdgeKind::ErrorToFunction) {
            error_to_fn.insert(e.src, e.dst);
        }
    }
    let mut fn_counts: BTreeMap<u32, usize> = BTreeMap::new();
    for (_, fn_id) in error_to_fn {
        *fn_counts.entry(fn_id).or_insert(0) += 1;
    }

    let mut entries: Vec<RepairSurfaceEntry> = Vec::new();
    for (fn_id, count) in fn_counts {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == fn_id) {
            entries.push(RepairSurfaceEntry {
                node_id: node.id,
                symbol: node.symbol.clone(),
                file: node.file.clone(),
                line: node.line,
                error_count: count,
            });
        }
    }
    entries.sort_by(|a, b| b.error_count.cmp(&a.error_count).then_with(|| a.symbol.cmp(&b.symbol)));
    entries
}

#[derive(Debug, Clone)]
struct ErrorSpan {
    file: String,
    line_start: u32,
    line_end: u32,
    col_start: u32,
    col_end: u32,
}

fn primary_span(err: &Value) -> Option<ErrorSpan> {
    let spans = err.get("spans")?.as_array()?;
    let span = spans.iter().find(|s| s.get("is_primary").and_then(|v| v.as_bool()).unwrap_or(false)).or_else(|| spans.first())?;
    let span = ErrorSpan {
        file: span.get("file_name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        line_start: span.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        line_end: span.get("line_end").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        col_start: span.get("column_start").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        col_end: span.get("column_end").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
    };
    let _ = span.col_end;
    Some(span)
}

fn find_nodes_at_span<'a>(nodes: &'a [Node], span: &ErrorSpan, kind: NodeKind) -> Vec<&'a Node> {
    let span_file = normalize_file(&span.file);
    nodes
        .iter()
        .filter(|n| n.kind == kind)
        .filter(|n| files_match(&normalize_file(&n.file), &span_file))
        .filter(|n| n.line >= span.line_start && n.line <= span.line_end)
        .collect()
}

fn find_nearest_node<'a>(nodes: &'a [Node], span: &ErrorSpan, kinds: &[NodeKind]) -> Option<&'a Node> {
    let span_file = normalize_file(&span.file);
    let mut candidates: Vec<&Node> = nodes
        .iter()
        .filter(|n| kinds.contains(&n.kind))
        .filter(|n| files_match(&normalize_file(&n.file), &span_file))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|n| {
        let line = n.line as i64;
        let target = span.line_start as i64;
        let diff = if line <= target { target - line } else { line - target + 1_000_000 };
        diff
    });
    candidates.first().copied()
}

fn files_match(node_file: &str, span_file: &str) -> bool {
    if node_file == span_file {
        return true;
    }
    node_file.ends_with(span_file) || span_file.ends_with(node_file)
}

fn normalize_file(raw: &str) -> String {
    if let Some(idx) = raw.find("embeddable_name: \"") {
        let start = idx + "embeddable_name: \"".len();
        if let Some(end) = raw[start..].find('"') {
            return raw[start..start + end].to_string();
        }
    }
    if let Some(idx) = raw.find("name: \"") {
        let start = idx + "name: \"".len();
        if let Some(end) = raw[start..].find('"') {
            return raw[start..start + end].to_string();
        }
    }
    raw.to_string()
}

fn read_nodes_csv(path: PathBuf) -> Result<Vec<Node>> {
    let content = fs::read_to_string(path.clone())?;
    let files = read_files_txt(path.parent().unwrap_or_else(|| Path::new(".")).join("files.txt"))?;
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            return Err(anyhow!("invalid nodes.csv line"));
        }
        let id = parts[0].parse::<u32>()?;
        let kind = parse_node_kind(parts[1]).map_err(|e| anyhow!(e))?;
        let line_no = parts[parts.len() - 3].parse::<u32>()?;
        let col = parts[parts.len() - 2].parse::<u32>()?;
        let file_id = parts[parts.len() - 4].parse::<usize>()?;
        let file = files.get(file_id).cloned().unwrap_or_default();
        let symbol = parts[2..parts.len() - 4].join(",");
        nodes.push(Node {
            id,
            kind,
            symbol,
            file,
            line: line_no,
            column: col,
            file_id: None,
            parent: None,
        });
    }
    Ok(nodes)
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

fn read_edges_csv(path: PathBuf) -> Result<Vec<Edge>> {
    let content = fs::read_to_string(path)?;
    let mut edges = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            return Err(anyhow!("invalid edges.csv line"));
        }
        let src = parts[0].parse::<u32>()?;
        let dst = parts[1].parse::<u32>()?;
        let kind = parse_edge_kind(parts[2]).map_err(|e| anyhow!(e))?;
        edges.push(Edge { src, dst, kind });
    }
    Ok(edges)
}
