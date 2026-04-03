use anyhow::{anyhow, Result};
use canon_analysis::graph_artifacts::{load_graph_artifact, GraphArtifactSummary};
use canon_graph::artifacts::artifact_writer::{
    emit_callgraph_csv, emit_cfg_csv, emit_graph_bin, emit_modulegraph_csv,
};
use canon_graph::graph::graph_types::{CodeGraphEdge, CodeGraphNode, ModuleNode};
use canon_ir::{CanonId, CanonIR, CanonNodeKind};
use canon_types::ReportLayout;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let workspace =
        arg_value(&args, "--workspace").unwrap_or("/workspace/ai_sandbox/canon".to_string());
    let index_dir = PathBuf::from(&workspace).join("state/graph/index/by_crate");

    // --list-crates: print available crates and exit
    if args.iter().any(|a| a == "--list-crates") {
        let crates = list_indexed_crates(&index_dir)?;
        if crates.is_empty() {
            eprintln!("no crates indexed at {}", index_dir.display());
        } else {
            for name in &crates {
                println!("{name}");
            }
        }
        return Ok(());
    }

    let crate_name = if let Some(name) = arg_value(&args, "--crate") {
        name
    } else if let Some(path) = arg_value(&args, "--crate-path") {
        crate_name_from_path(Path::new(&path))?
    } else {
        return Err(anyhow!(
            "usage: graph_bin --crate <name> [--workspace <path>] [--out <dir>] [--json] [--history] [--version <prefix>]\n\
             list available crates: graph_bin --list-crates"
        ));
    };

    // --history: list all captured versions for this crate and exit
    if args.iter().any(|a| a == "--history") {
        return print_history(&index_dir, &crate_name);
    }

    let emit_json = args.iter().any(|a| a == "--json");
    let version_prefix = arg_value(&args, "--version");

    let summary = load_summary(&index_dir, &crate_name, version_prefix.as_deref())?;

    if !summary.artifact_path.exists() {
        return Err(anyhow!(
            "artifact file not found: {}",
            summary.artifact_path.display()
        ));
    }

    let out_dir = arg_value(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(&workspace)
                .join("state/reports_out/crates")
                .join(&crate_name)
        });

    eprintln!(
        "[graph_bin] crate={crate_name} artifact={} out={}",
        summary.artifact_path.display(),
        out_dir.display()
    );

    let ir = load_graph_artifact(&summary.artifact_path)?;

    let layout = ReportLayout::from_direct_root(out_dir.clone());
    layout.ensure_dirs()?;

    let graph_dir = layout.graph_dir();
    let graphs_dir = layout.graphs_dir();
    let _ = fs::create_dir_all(&graph_dir);
    let _ = fs::create_dir_all(&graphs_dir);

    // Build CodeGraphProjection for graph.bin
    let (nodes, edges, files) = ir_to_projection(&ir);

    let graph_bin_path = graph_dir.join("graph.bin");
    emit_graph_bin(&graph_bin_path, &nodes, &edges, &files)?;

    // Call graph CSV — always from the complete CanonIR call_graph CSR
    let call_edges = csr_edges(&ir.call_graph);
    emit_callgraph_csv(&graphs_dir, &call_edges, &nodes, &files)?;

    // CFG CSV
    let cfg_edges: Vec<CodeGraphEdge> = csr_edges(&ir.cfg_graph)
        .into_iter()
        .map(|(s, d)| CodeGraphEdge { src: s, dst: d, kind: "CFG_EDGE".to_string() })
        .collect();
    emit_cfg_csv(&graphs_dir, &cfg_edges)?;

    // Module graph CSV
    let (module_edges, module_nodes) = extract_module_graph(&ir);
    emit_modulegraph_csv(&graphs_dir, &module_edges, &module_nodes)?;

    // Verify extracted counts match index metadata (completeness check)
    verify_counts(&summary, &call_edges, &cfg_edges, &module_edges)?;

    // Optionally write graph.json alongside graph.bin
    if emit_json {
        let json_path = graph_dir.join("graph.json");
        write_graph_json(
            &json_path,
            &crate_name,
            &summary,
            &nodes,
            &call_edges,
            &cfg_edges,
            &module_edges,
        )?;
        eprintln!("[graph_bin] wrote {}", json_path.display());
    }

    // Print summary
    println!("crate:      {crate_name}");
    println!("artifact:   {}", summary.artifact_id);
    println!("nodes:      {}", nodes.len());
    println!("call_edges: {} (index: {})", call_edges.len(), summary.call_edge_count);
    println!("cfg_edges:  {} (index: {})", cfg_edges.len(), summary.cfg_edge_count);
    println!("mod_edges:  {} (index: {})", module_edges.len(), summary.module_edge_count);
    println!("graph.bin:  {}", graph_bin_path.display());
    print_generated_outputs(&layout);
    Ok(())
}

// ── Count verification ───────────────────────────────────────────────────────

/// Verify that extracted edge counts match the index metadata.
/// The index is written by canon-rustc at capture time and reflects the
/// complete graph, so mismatches indicate a corrupt or truncated artifact.
fn verify_counts(
    summary: &GraphArtifactSummary,
    call_edges: &[(u32, u32)],
    cfg_edges: &[CodeGraphEdge],
    module_edges: &[(u32, u32)],
) -> Result<()> {
    let mut errs: Vec<String> = Vec::new();
    if call_edges.len() != summary.call_edge_count {
        errs.push(format!(
            "call_edges: extracted {} but index says {}",
            call_edges.len(),
            summary.call_edge_count
        ));
    }
    if cfg_edges.len() != summary.cfg_edge_count {
        errs.push(format!(
            "cfg_edges: extracted {} but index says {}",
            cfg_edges.len(),
            summary.cfg_edge_count
        ));
    }
    if module_edges.len() != summary.module_edge_count {
        errs.push(format!(
            "module_edges: extracted {} but index says {}",
            module_edges.len(),
            summary.module_edge_count
        ));
    }
    if !errs.is_empty() {
        return Err(anyhow!(
            "graph completeness mismatch for artifact {}:\n{}",
            summary.artifact_id,
            errs.join("\n")
        ));
    }
    Ok(())
}

// ── History access ───────────────────────────────────────────────────────────

/// Load the summary for a crate. If `version_prefix` is given, find the
/// history entry whose artifact_id starts with that prefix (for pinning to
/// a specific past complete snapshot). Otherwise use the latest index.
fn load_summary(
    index_dir: &Path,
    crate_name: &str,
    version_prefix: Option<&str>,
) -> Result<GraphArtifactSummary> {
    let index_path = index_dir.join(format!("{crate_name}.json"));
    if !index_path.exists() {
        let available = list_indexed_crates(index_dir).unwrap_or_default();
        return Err(anyhow!(
            "no artifact index for crate '{crate_name}' (expected: {})\navailable: {}",
            index_path.display(),
            if available.is_empty() { "<none>".to_string() } else { available.join(", ") }
        ));
    }

    if let Some(prefix) = version_prefix {
        // Search history for a matching artifact_id prefix
        let history_path = index_dir.join(format!("{crate_name}.history.jsonl"));
        if !history_path.exists() {
            return Err(anyhow!(
                "no history file for crate '{crate_name}'; cannot resolve version '{prefix}'"
            ));
        }
        let content = fs::read_to_string(&history_path)?;
        // Iterate in reverse so latest matching version wins on ambiguous prefix
        let mut found: Option<GraphArtifactSummary> = None;
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: GraphArtifactSummary = serde_json::from_str(line)
                .map_err(|e| anyhow!("history parse error: {e}"))?;
            if entry.artifact_id.starts_with(prefix) {
                found = Some(entry);
                break;
            }
        }
        return found.ok_or_else(|| {
            anyhow!(
                "no history entry for crate '{crate_name}' with artifact_id prefix '{prefix}'"
            )
        });
    }

    let summary: GraphArtifactSummary = serde_json::from_slice(&fs::read(&index_path)?)
        .map_err(|e| anyhow!("failed to parse index {}: {e}", index_path.display()))?;
    Ok(summary)
}

/// Print all historical snapshots for a crate (newest last).
fn print_history(index_dir: &Path, crate_name: &str) -> Result<()> {
    let history_path = index_dir.join(format!("{crate_name}.history.jsonl"));
    if !history_path.exists() {
        eprintln!("no history for crate '{crate_name}'");
        return Ok(());
    }
    let content = fs::read_to_string(&history_path)?;
    // Deduplicate by artifact_id, preserving latest occurrence
    let mut seen_ids = std::collections::LinkedList::new();
    let mut entries: std::collections::HashMap<String, GraphArtifactSummary> =
        std::collections::HashMap::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<GraphArtifactSummary>(line) else {
            continue;
        };
        if !entries.contains_key(&entry.artifact_id) {
            seen_ids.push_back(entry.artifact_id.clone());
        }
        entries.insert(entry.artifact_id.clone(), entry);
    }
    println!("history for {crate_name} ({} distinct versions):", seen_ids.len());
    for id in &seen_ids {
        let e = &entries[id];
        let ts = if e.captured_at_ms > 0 {
            format!(" captured_at_ms={}", e.captured_at_ms)
        } else {
            String::new()
        };
        let exists = if e.artifact_path.exists() { "" } else { " [MISSING]" };
        println!(
            "  {} nodes={} call={} cfg={} mod={}{ts}{exists}",
            &id[..id.len().min(16)],
            e.node_count,
            e.call_edge_count,
            e.cfg_edge_count,
            e.module_edge_count,
        );
    }
    Ok(())
}

// ── JSON graph output ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct GraphJson<'a> {
    crate_name: &'a str,
    artifact_id: &'a str,
    node_count: usize,
    call_edge_count: usize,
    cfg_edge_count: usize,
    module_edge_count: usize,
    nodes: Vec<NodeJson<'a>>,
    call_edges: Vec<[u32; 2]>,
    cfg_edges: Vec<[u32; 2]>,
    module_edges: Vec<[u32; 2]>,
}

#[derive(Serialize)]
struct NodeJson<'a> {
    id: u32,
    kind: &'a str,
    symbol: &'a str,
}

fn write_graph_json(
    path: &Path,
    crate_name: &str,
    summary: &GraphArtifactSummary,
    nodes: &[CodeGraphNode],
    call_edges: &[(u32, u32)],
    cfg_edges: &[CodeGraphEdge],
    module_edges: &[(u32, u32)],
) -> Result<()> {
    let doc = GraphJson {
        crate_name,
        artifact_id: &summary.artifact_id,
        node_count: nodes.len(),
        call_edge_count: call_edges.len(),
        cfg_edge_count: cfg_edges.len(),
        module_edge_count: module_edges.len(),
        nodes: nodes
            .iter()
            .map(|n| NodeJson { id: n.id, kind: &n.kind, symbol: &n.symbol })
            .collect(),
        call_edges: call_edges.iter().map(|&(s, d)| [s, d]).collect(),
        cfg_edges: cfg_edges.iter().map(|e| [e.src, e.dst]).collect(),
        module_edges: module_edges.iter().map(|&(s, d)| [s, d]).collect(),
    };
    let json = serde_json::to_vec_pretty(&doc)?;
    fs::write(path, json)?;
    Ok(())
}

// ── CanonIR → CodeGraphProjection ──────────────────────────────────────────

fn ir_to_projection(ir: &CanonIR) -> (Vec<CodeGraphNode>, Vec<CodeGraphEdge>, Vec<String>) {
    let files: Vec<String> = Vec::new();

    let nodes: Vec<CodeGraphNode> = ir
        .nodes
        .iter()
        .map(|n| {
            let (kind, symbol) = node_kind_symbol(ir, &n.kind, n.id.0);
            CodeGraphNode { id: n.id.0, kind, symbol, file_id: None, line: None }
        })
        .collect();

    let mut edges: Vec<CodeGraphEdge> = Vec::new();
    for (src, dst) in csr_edges(&ir.call_graph) {
        edges.push(CodeGraphEdge { src, dst, kind: "CALL".to_string() });
    }
    for (src, dst) in csr_edges(&ir.cfg_graph) {
        edges.push(CodeGraphEdge { src, dst, kind: "CFG_EDGE".to_string() });
    }
    for (src, dst) in csr_edges(&ir.module_graph) {
        edges.push(CodeGraphEdge { src, dst, kind: "MODULE".to_string() });
    }
    for (src, dst) in csr_edges(&ir.type_graph) {
        edges.push(CodeGraphEdge { src, dst, kind: "TYPE".to_string() });
    }
    for (src, dst) in csr_edges(&ir.name_graph) {
        edges.push(CodeGraphEdge { src, dst, kind: "NAME".to_string() });
    }

    (nodes, edges, files)
}

/// Extract all (src_canon_id, dst_canon_id) edges from a CSR graph.
fn csr_edges<ED>(graph: &canon_ir::csr_graph::CsrGraph<CanonId, ED>) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let nd = &graph.node_data;
    let rp = &graph.row_ptr;
    let ci = &graph.col_idx;
    for src_row in 0..nd.len() {
        let start = rp[src_row] as usize;
        let end = rp[src_row + 1] as usize;
        for e in start..end {
            let dst_row = ci[e] as usize;
            out.push((nd[src_row].0, nd[dst_row].0));
        }
    }
    out
}

fn extract_module_graph(ir: &CanonIR) -> (Vec<(u32, u32)>, Vec<ModuleNode>) {
    let graph = &ir.module_graph;
    let nd = &graph.node_data;
    let rp = &graph.row_ptr;
    let ci = &graph.col_idx;

    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut module_nodes: Vec<ModuleNode> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    let add_module_node = |id: u32,
                           ir: &CanonIR,
                           seen: &mut std::collections::HashSet<u32>,
                           out: &mut Vec<ModuleNode>| {
        if !seen.insert(id) {
            return;
        }
        if let Some(node) = ir.nodes.get(id as usize) {
            let symbol = match &node.kind {
                CanonNodeKind::Module { path_id, .. } => {
                    ir.lookup_path(*path_id).to_string()
                }
                CanonNodeKind::Crate { name_id, .. } => {
                    format!("crate::{}", ir.lookup_name(*name_id))
                }
                _ => format!("node_{id}"),
            };
            out.push(ModuleNode { id, symbol, file: String::new() });
        }
    };

    for src_row in 0..nd.len() {
        let src_id = nd[src_row].0;
        let start = rp[src_row] as usize;
        let end = rp[src_row + 1] as usize;
        for e in start..end {
            let dst_row = ci[e] as usize;
            let dst_id = nd[dst_row].0;
            edges.push((src_id, dst_id));
            add_module_node(src_id, ir, &mut seen_ids, &mut module_nodes);
            add_module_node(dst_id, ir, &mut seen_ids, &mut module_nodes);
        }
    }

    (edges, module_nodes)
}

fn node_kind_symbol(ir: &CanonIR, kind: &CanonNodeKind, id: u32) -> (String, String) {
    match kind {
        CanonNodeKind::Crate { name_id, .. } => {
            ("CRATE".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Module { path_id, .. } => {
            ("MODULE".into(), ir.lookup_path(*path_id).to_string())
        }
        CanonNodeKind::Struct { name_id, .. } => {
            ("STRUCT".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Enum { name_id, .. } => {
            ("ENUM".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Trait { name_id, .. } => {
            ("TRAIT".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::AssocType { name_id, .. } => {
            ("ASSOC_TYPE".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::AssocConst { name_id, .. } => {
            ("ASSOC_CONST".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Fn { name_id, .. } => {
            ("FN".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Impl { .. } => ("IMPL".into(), format!("impl_{id}")),
        CanonNodeKind::FnSig { .. } => ("FN_SIG".into(), format!("sig_{id}")),
        CanonNodeKind::Type { .. } => ("TYPE".into(), format!("type_{id}")),
        CanonNodeKind::Field { name_id, .. } => {
            let sym = name_id
                .map(|n| ir.lookup_name(n).to_string())
                .unwrap_or_else(|| format!("field_{id}"));
            ("FIELD".into(), sym)
        }
        CanonNodeKind::Param { name_id, .. } => {
            ("PARAM".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::GenericParam { name_id, .. } => {
            ("GENERIC_PARAM".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Variant { name_id, .. } => {
            ("VARIANT".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Const { name_id, .. } => {
            ("CONST".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::Static { name_id, .. } => {
            ("STATIC".into(), ir.lookup_name(*name_id).to_string())
        }
        CanonNodeKind::TypeAlias { name_id, .. } => {
            ("TYPE_ALIAS".into(), ir.lookup_name(*name_id).to_string())
        }
        _ => ("OTHER".into(), format!("node_{id}")),
    }
}

// ── Index listing ───────────────────────────────────────────────────────────

fn list_indexed_crates(index_dir: &Path) -> Result<Vec<String>> {
    if !index_dir.exists() {
        return Ok(Vec::new());
    }
    let mut crates = std::collections::BTreeSet::new();
    for entry in fs::read_dir(index_dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let stem = match p.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if stem.contains('.') {
            continue; // skip foo.history stems
        }
        crates.insert(stem);
    }
    Ok(crates.into_iter().collect())
}

// ── Output summary ──────────────────────────────────────────────────────────

fn print_generated_outputs(layout: &ReportLayout) {
    let sections = [
        ("graph", layout.graph_dir()),
        ("graphs", layout.graphs_dir()),
        ("analysis", layout.analysis_dir()),
        ("metrics", layout.metrics_dir()),
    ];
    for (label, dir) in &sections {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        if files.is_empty() {
            continue;
        }
        files.sort();
        println!("{label}/");
        for f in files {
            println!("  {f}");
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].to_string())
}

fn crate_name_from_path(crate_path: &Path) -> Result<String> {
    let manifest = crate_path.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest)
        .map_err(|e| anyhow!("failed to read {}: {}", manifest.display(), e))?;
    let mut in_package = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('=').map(str::trim) {
                let name = value.trim_matches('"');
                if !name.is_empty() {
                    return Ok(name.replace('-', "_"));
                }
            }
        }
    }
    Err(anyhow!("package name not found in {}", manifest.display()))
}
