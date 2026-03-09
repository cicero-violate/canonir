use crate::csr::build_csr;
use crate::emit::OutputConfig;
use crate::types::{Edge, EdgeKind, Metadata, Node, NodeKind, SpanRange};
use anyhow::Result;
use rustc_hir as hir;
use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::{DefId, LOCAL_CRATE, CRATE_DEF_ID};
use rustc_span::Pos;
use rustc_span::symbol::sym;
use std::collections::{BTreeMap, HashMap};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct UpgGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub csr: crate::csr::CsrGraph,
    pub metadata: Metadata,
    pub spans_primary: Vec<SpanRange>,
    pub def_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct NodeSpec {
    kind: NodeKind,
    kind_id: u32,
    symbol: String,
    file: String,
    line: u32,
    column: u32,
    span_lo: u32,
    span_hi: u32,
}

pub fn extract_and_write(tcx: TyCtxt<'_>, config: &OutputConfig) -> Result<UpgGraph> {
    let graph = extract_upg(tcx, &config.output_dir)
        .map_err(|err| anyhow::anyhow!("extract_upg failed: {err}"))?;
    let merged = merge_with_existing(&config.output_dir, graph)
        .map_err(|err| anyhow::anyhow!("merge_with_existing failed: {err}"))?;
    crate::emit::write_outputs(&merged, &config.output_dir)
        .map_err(|err| anyhow::anyhow!("write_outputs failed: {err}"))?;
    Ok(merged)
}

pub fn extract_upg(tcx: TyCtxt<'_>, output_dir: &Path) -> Result<UpgGraph> {
    let project = output_dir
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| output_dir.display().to_string());

    let mut node_specs: Vec<NodeSpec> = Vec::new();
    let mut def_paths: Vec<(DefId, String)> = Vec::new();

    for def_id in local_def_ids(tcx) {
        let path = tcx.def_path_str(def_id);
        def_paths.push((def_id, path.clone()));
        node_specs.extend(structure_nodes_for_def(tcx, def_id, &path));
        node_specs.extend(mir_nodes_for_def(tcx, def_id, &path)?);
    }

    node_specs.retain(|spec| is_project_file(output_dir, &spec.file));
    let mut unique: BTreeMap<(String, u32), NodeSpec> = BTreeMap::new();
    for spec in node_specs {
        unique.entry((spec.symbol.clone(), spec.kind_id)).or_insert(spec);
    }
    let mut node_specs: Vec<NodeSpec> = unique.into_values().collect();
    node_specs.sort_by(|a, b| a.symbol.cmp(&b.symbol).then_with(|| a.kind_id.cmp(&b.kind_id)));
    let mut nodes: Vec<Node> = Vec::with_capacity(node_specs.len());
    let mut spans_primary: Vec<SpanRange> = Vec::with_capacity(node_specs.len());
    for (idx, spec) in node_specs.into_iter().enumerate() {
        let id = idx as u32;
        nodes.push(Node {
            id,
            kind: spec.kind,
            symbol: spec.symbol,
            file: spec.file,
            line: spec.line,
            column: spec.column,
        });
        spans_primary.push(SpanRange {
            lo: spec.span_lo,
            hi: spec.span_hi,
        });
    }

    let (nodes, spans_primary, symbol_to_id) = dedup_nodes(nodes, spans_primary);
    let (nodes, spans_primary, symbol_to_id) =
        ensure_fn_nodes_for_bb(nodes, spans_primary, symbol_to_id);

    let mut edges = build_edges(tcx, &def_paths, &symbol_to_id)?;
    let id_to_kind: HashMap<u32, NodeKind> = nodes
        .iter()
        .map(|n| (n.id, n.kind))
        .collect();
    add_call_edges_from_mir(tcx, &def_paths, &symbol_to_id, &mut edges);
    add_callsite_block_edges(&nodes, &symbol_to_id, &mut edges);
    add_module_contains_edges_from_nodes(tcx, &nodes, &symbol_to_id, &mut edges);
    add_module_import_edges(tcx, &nodes, &symbol_to_id, &mut edges);
    add_export_edges(tcx, &def_paths, &symbol_to_id, &id_to_kind, &mut edges);
    add_public_use_edges(tcx, &symbol_to_id, &id_to_kind, &mut edges);
    add_contains_for_isolated_types(tcx, &nodes, &symbol_to_id, &mut edges);
    let csr = build_csr(nodes.len() as u32, &edges);
    let def_paths_out: Vec<String> = def_paths
        .iter()
        .filter_map(|(def_id, path)| {
            let (def_file, _, _, _, _) = span_info(tcx, tcx.def_span(*def_id));
            if !is_project_file(output_dir, &def_file) {
                return None;
            }
            match tcx.def_kind(*def_id) {
                rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn => {
                    Some(format!("{path}::fn"))
                }
                rustc_hir::def::DefKind::Struct
                | rustc_hir::def::DefKind::Enum
                | rustc_hir::def::DefKind::Trait
                | rustc_hir::def::DefKind::Mod => Some(path.clone()),
                rustc_hir::def::DefKind::Impl { .. } => Some(impl_symbol(path)),
                _ => None,
            }
        })
        .collect();
    let metadata = Metadata {
        project,
        node_count: nodes.len() as u32,
        edge_count: csr.col_idx.len() as u32,
        def_count: def_paths_out.len() as u32,
        generated_by: "UPG extractor".to_string(),
    };

    Ok(UpgGraph {
        nodes,
        edges,
        csr,
        metadata,
        spans_primary,
        def_paths: def_paths_out,
    })
}

fn merge_with_existing(output_dir: &Path, graph: UpgGraph) -> Result<UpgGraph> {
    let nodes_path = output_dir.join("nodes.csv");
    let edges_path = output_dir.join("edges.csv");
    if !nodes_path.exists() || !edges_path.exists() {
        return Ok(graph);
    }
    // Prefer fresh graph output; stale CSV entries should not be preserved.
    Ok(graph)
}

fn dedup_nodes(
    mut nodes: Vec<Node>,
    spans: Vec<SpanRange>,
) -> (Vec<Node>, Vec<SpanRange>, BTreeMap<String, u32>) {
    for node in &mut nodes {
        if node.kind == NodeKind::Impl && !node.symbol.ends_with("::impl") {
            node.symbol = impl_symbol(&node.symbol);
        }
    }
    let mut seen: std::collections::HashSet<(NodeKind, String)> = std::collections::HashSet::new();
    let mut keep_indices: Vec<usize> = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        let key = (node.kind, node.symbol.clone());
        if seen.insert(key) {
            keep_indices.push(idx);
        }
    }
    let mut new_nodes: Vec<Node> = Vec::with_capacity(keep_indices.len());
    let mut new_spans: Vec<SpanRange> = Vec::with_capacity(keep_indices.len());
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    for (new_id, &old_idx) in keep_indices.iter().enumerate() {
        let mut node = nodes[old_idx].clone();
        node.id = new_id as u32;
        symbol_to_id.insert(node.symbol.clone(), node.id);
        new_nodes.push(node);
        new_spans.push(spans[old_idx]);
    }
    (new_nodes, new_spans, symbol_to_id)
}

fn read_nodes_csv(output_dir: &Path, path: std::path::PathBuf) -> Result<Vec<Node>> {
    let content = std::fs::read_to_string(path)?;
    let header = content.lines().next().unwrap_or_default();
    let has_file_id = header.contains("file_id");
    let files = read_files_txt(output_dir.join("files.txt")).unwrap_or_default();
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if has_file_id && parts.len() < 7 {
            continue;
        }
        if !has_file_id && parts.len() < 6 {
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
        let (file, line_no, col, symbol) = if has_file_id {
            let line_no = match parts[parts.len() - 3].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let col = match parts[parts.len() - 2].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let file_id = match parts[parts.len() - 4].parse::<usize>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let file = files.get(file_id).cloned().unwrap_or_default();
            let symbol = parts[2..parts.len() - 4].join(",");
            (file, line_no, col, symbol)
        } else {
            let line_no = match parts[parts.len() - 2].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let col = match parts[parts.len() - 1].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let file = parts[parts.len() - 3].to_string();
            let symbol = parts[2..parts.len() - 3].join(",");
            (file, line_no, col, symbol)
        };
        nodes.push(Node {
            id,
            kind,
            symbol,
            file,
            line: line_no,
            column: col,
        });
    }
    Ok(nodes)
}

fn read_files_txt(path: std::path::PathBuf) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    let mut files = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }
        let id = match parts[0].parse::<usize>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let path = parts[1..].join(",");
        if files.len() <= id {
            files.resize(id + 1, String::new());
        }
        files[id] = path;
    }
    Ok(files)
}

fn read_edges_csv(path: std::path::PathBuf) -> Result<Vec<Edge>> {
    let content = std::fs::read_to_string(path)?;
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
        let kind = match parse_edge_kind(parts[2]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        edges.push(Edge { src, dst, kind });
    }
    Ok(edges)
}

fn parse_node_kind(raw: &str) -> Result<NodeKind> {
    match raw {
        "FUNCTION" => Ok(NodeKind::Function),
        "METHOD" => Ok(NodeKind::Method),
        "STRUCT" => Ok(NodeKind::Struct),
        "ENUM" => Ok(NodeKind::Enum),
        "TRAIT" => Ok(NodeKind::Trait),
        "IMPL" => Ok(NodeKind::Impl),
        "FIELD" => Ok(NodeKind::Field),
        "PARAM" => Ok(NodeKind::Param),
        "VARIABLE" => Ok(NodeKind::Variable),
        "MODULE" => Ok(NodeKind::Module),
        "TYPE" => Ok(NodeKind::Type),
        "BASIC_BLOCK" => Ok(NodeKind::BasicBlock),
        "CALL_SITE" => Ok(NodeKind::CallSite),
        "ERROR" => Ok(NodeKind::Error),
        _ => Err(anyhow::anyhow!("unknown node kind")),
    }
}

fn parse_edge_kind(raw: &str) -> Result<EdgeKind> {
    match raw {
        "CONTAINS" => Ok(EdgeKind::Contains),
        "HAS_FIELD" => Ok(EdgeKind::HasField),
        "HAS_METHOD" => Ok(EdgeKind::HasMethod),
        "HAS_BLOCK" => Ok(EdgeKind::HasBlock),
        "HAS_PARAM" => Ok(EdgeKind::HasParam),
        "IMPORTS" => Ok(EdgeKind::Imports),
        "EXPORT" => Ok(EdgeKind::Export),
        "PUBLIC_USE" => Ok(EdgeKind::PublicUse),
        "FLOW" => Ok(EdgeKind::Flow),
        "CALL" => Ok(EdgeKind::Call),
        "RETURN" => Ok(EdgeKind::Return),
        "UNWIND" => Ok(EdgeKind::Unwind),
        "IMPLEMENTS" => Ok(EdgeKind::Implements),
        "FOR_TYPE" => Ok(EdgeKind::ForType),
        "USES_TYPE" => Ok(EdgeKind::UsesType),
        "BOUNDS" => Ok(EdgeKind::Bounds),
        "ASSIGN" => Ok(EdgeKind::Assign),
        "PROPAGATES" => Ok(EdgeKind::Propagates),
        "ARG_TO_PARAM" => Ok(EdgeKind::ArgToParam),
        "RETURNS" => Ok(EdgeKind::Returns),
        "ERROR_TO_FUNCTION" => Ok(EdgeKind::ErrorToFunction),
        "ERROR_TO_BLOCK" => Ok(EdgeKind::ErrorToBlock),
        _ => Err(anyhow::anyhow!("unknown edge kind")),
    }
}

fn structure_nodes_for_def(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<NodeSpec> {
    let mut nodes = Vec::new();
    let span = tcx.def_span(def_id);
    let (file, line, column, span_lo, span_hi) = span_info(tcx, span);
    let mir_available = def_id
        .as_local()
        .map(|local| tcx.is_mir_available(local))
        .unwrap_or(false);

    match tcx.def_kind(def_id) {
        rustc_hir::def::DefKind::Fn => {
            if !mir_available {
                nodes.push(NodeSpec {
                    kind: NodeKind::Function,
                    kind_id: 1,
                    symbol: format!("{path}::fn"),
                    file,
                    line,
                    column,
                    span_lo,
                    span_hi,
                });
            }
            nodes.extend(param_nodes(tcx, def_id, path));
        }
        rustc_hir::def::DefKind::AssocFn => {
            if !mir_available {
                nodes.push(NodeSpec {
                    kind: NodeKind::Method,
                    kind_id: 2,
                    symbol: format!("{path}::fn"),
                    file,
                    line,
                    column,
                    span_lo,
                    span_hi,
                });
            }
            nodes.extend(param_nodes(tcx, def_id, path));
        }
        rustc_hir::def::DefKind::Struct => {
            nodes.push(NodeSpec {
                kind: NodeKind::Struct,
                kind_id: 3,
                symbol: path.to_string(),
                file,
                line,
                column,
                span_lo,
                span_hi,
            });
            nodes.extend(field_nodes(tcx, def_id, path));
        }
        rustc_hir::def::DefKind::Enum => {
            nodes.push(NodeSpec {
                kind: NodeKind::Enum,
                kind_id: 4,
                symbol: path.to_string(),
                file,
                line,
                column,
                span_lo,
                span_hi,
            });
            nodes.extend(field_nodes(tcx, def_id, path));
        }
        rustc_hir::def::DefKind::Trait => {
            nodes.push(NodeSpec {
                kind: NodeKind::Trait,
                kind_id: 5,
                symbol: path.to_string(),
                file,
                line,
                column,
                span_lo,
                span_hi,
            });
        }
        rustc_hir::def::DefKind::Impl { .. } => {
            nodes.push(NodeSpec {
                kind: NodeKind::Impl,
                kind_id: 6,
                symbol: impl_symbol(path),
                file,
                line,
                column,
                span_lo,
                span_hi,
            });
        }
        rustc_hir::def::DefKind::Mod => {
            let mut mod_file = file.clone();
            let mut mod_line = line;
            let mut mod_column = column;
            let mut mod_span_lo = span_lo;
            let mut mod_span_hi = span_hi;
            if let Some(mod_name) = path.rsplit("::").next() {
                if mod_name != "crate" {
                    let path_attr = module_path_attr_value(tcx, def_id);
                    if let Some(resolved) =
                        resolve_module_file_from_decl(&file, mod_name, path_attr.as_deref())
                    {
                        mod_file = resolved;
                        mod_line = 1;
                        mod_column = 1;
                        mod_span_lo = 0;
                        mod_span_hi = 0;
                    }
                }
            }
            nodes.push(NodeSpec {
                kind: NodeKind::Module,
                kind_id: 7,
                symbol: path.to_string(),
                file: mod_file,
                line: mod_line,
                column: mod_column,
                span_lo: mod_span_lo,
                span_hi: mod_span_hi,
            });
        }
        _ => {}
    }

    nodes
}

fn mir_nodes_for_def(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Result<Vec<NodeSpec>> {
    let Some(local_def) = def_id.as_local() else {
        return Ok(Vec::new());
    };
    if !tcx.is_mir_available(local_def) {
        return Ok(Vec::new());
    }
    let body = match tcx.hir_body_const_context(local_def) {
        Some(hir::ConstContext::ConstFn)
        | Some(hir::ConstContext::Const { .. })
        | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };
    let (fn_file, fn_line, fn_column, fn_span_lo, fn_span_hi) = span_info(tcx, body.span);
    let mut nodes = Vec::new();
    let def_kind = tcx.def_kind(def_id);
    let (fn_kind, fn_kind_id) = match def_kind {
        rustc_hir::def::DefKind::AssocFn => (NodeKind::Method, 2),
        rustc_hir::def::DefKind::Fn => (NodeKind::Function, 1),
        _ => (NodeKind::Function, 1),
    };
    nodes.push(NodeSpec {
        kind: fn_kind,
        kind_id: fn_kind_id,
        symbol: format!("{path}::fn"),
        file: fn_file.clone(),
        line: fn_line,
        column: fn_column,
        span_lo: fn_span_lo,
        span_hi: fn_span_hi,
    });
    for (idx, _bb) in body.basic_blocks.iter().enumerate() {
        nodes.push(NodeSpec {
            kind: NodeKind::BasicBlock,
            kind_id: 20,
            symbol: format!("{path}::bb{idx}"),
            file: fn_file.clone(),
            line: fn_line,
            column: fn_column,
            span_lo: fn_span_lo,
            span_hi: fn_span_hi,
        });
    }

    let mut call_index = 0usize;
    for (idx, bb) in body.basic_blocks.iter().enumerate() {
        if let Some(term) = &bb.terminator {
            if matches!(term.kind, mir::TerminatorKind::Call { .. }) {
                nodes.push(NodeSpec {
                    kind: NodeKind::CallSite,
                    kind_id: 21,
                    symbol: format!("{path}::bb{idx}::call{call_index}"),
                    file: fn_file.clone(),
                    line: fn_line,
                    column: fn_column,
                    span_lo: fn_span_lo,
                    span_hi: fn_span_hi,
                });
                call_index += 1;
            }
        }
    }

    Ok(nodes)
}

fn build_edges(
    tcx: TyCtxt<'_>,
    def_paths: &[(DefId, String)],
    symbol_to_id: &BTreeMap<String, u32>,
) -> Result<Vec<Edge>> {
    let mut edges = Vec::new();
    let add_contains = |item_path: &str, item_id: u32, edges: &mut Vec<Edge>| {
        if item_path.is_empty() {
            return;
        }
        let parent_path = item_path
            .rsplitn(2, "::")
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| "crate".to_string());
        if let Some(parent_id) = resolve_parent_module_id(tcx, &parent_path, symbol_to_id) {
            edges.push(Edge {
                src: parent_id,
                dst: item_id,
                kind: EdgeKind::Contains,
            });
        }
    };

    for (def_id, path) in def_paths {
        match tcx.def_kind(*def_id) {
            rustc_hir::def::DefKind::Struct | rustc_hir::def::DefKind::Enum => {
                let parent_id = symbol_to_id.get(path).copied();
                if let Some(parent_id) = parent_id {
                    add_contains(path, parent_id, &mut edges);
                    for field in field_symbols(tcx, *def_id, path) {
                        if let Some(field_id) = symbol_to_id.get(&field) {
                            edges.push(Edge {
                                src: parent_id,
                                dst: *field_id,
                                kind: EdgeKind::HasField,
                            });
                        }
                    }
                }
            }
            rustc_hir::def::DefKind::Trait => {
                let trait_id = symbol_to_id.get(path).copied();
                if let Some(trait_id) = trait_id {
                    add_contains(path, trait_id, &mut edges);
                }
            }
            rustc_hir::def::DefKind::Impl { of_trait } => {
                let impl_id = symbol_to_id.get(&impl_symbol(path)).copied();
                if let Some(impl_id) = impl_id {
                    add_contains(path, impl_id, &mut edges);
                    if of_trait {
                        let trait_ref = tcx.impl_trait_ref(*def_id).skip_binder();
                        let trait_path = tcx.def_path_str(trait_ref.def_id);
                        if let Some(trait_id) = symbol_to_id.get(&trait_path) {
                            edges.push(Edge {
                                src: impl_id,
                                dst: *trait_id,
                                kind: EdgeKind::Implements,
                            });
                        }
                    }
                    let self_ty = tcx.type_of(*def_id).skip_binder();
                    if let rustc_middle::ty::TyKind::Adt(adt, _) = self_ty.kind() {
                        let self_path = tcx.def_path_str(adt.did());
                        if let Some(self_id) = symbol_to_id.get(&self_path) {
                            edges.push(Edge {
                                src: impl_id,
                                dst: *self_id,
                                kind: EdgeKind::ForType,
                            });
                        }
                    }
                }
            }
            rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn => {
                let fn_symbol = format!("{path}::fn");
                let fn_id = symbol_to_id.get(&fn_symbol).copied();
                if let Some(fn_id) = fn_id {
                    add_contains(path, fn_id, &mut edges);
                    for param_symbol in param_symbols(tcx, *def_id, path) {
                        if let Some(param_id) = symbol_to_id.get(&param_symbol) {
                            edges.push(Edge {
                                src: fn_id,
                                dst: *param_id,
                                kind: EdgeKind::HasParam,
                            });
                        }
                    }
                }
            }
            rustc_hir::def::DefKind::Mod => {
                if path.is_empty() {
                    continue;
                }
                if let Some(child_id) = symbol_to_id.get(path).copied() {
                    let parent_path = path
                        .rsplitn(2, "::")
                        .nth(1)
                        .map(str::to_string)
                        .unwrap_or_else(|| "crate".to_string());
                    if let Some(parent_id) =
                        resolve_parent_module_id(tcx, &parent_path, symbol_to_id)
                    {
                        if parent_id == child_id {
                            continue;
                        }
                        edges.push(Edge {
                            src: parent_id,
                            dst: child_id,
                            kind: EdgeKind::Imports,
                        });
                    }
                }
            }
            _ => {}
        }

        let Some(local_def) = def_id.as_local() else {
            continue;
        };
        if !tcx.is_mir_available(local_def) {
            continue;
        }
        let body = match tcx.hir_body_const_context(local_def) {
            Some(hir::ConstContext::ConstFn)
            | Some(hir::ConstContext::Const { .. })
            | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
            None => tcx.optimized_mir(local_def),
        };

        let fn_symbol = format!("{path}::fn");
        let fn_id = symbol_to_id.get(&fn_symbol).copied();

        let mut call_index = 0usize;
        for (idx, bb) in body.basic_blocks.iter().enumerate() {
            let bb_symbol = format!("{path}::bb{idx}");
            let bb_id = match symbol_to_id.get(&bb_symbol) {
                Some(id) => *id,
                None => continue,
            };
            if let Some(fn_id) = fn_id {
                edges.push(Edge {
                    src: fn_id,
                    dst: bb_id,
                    kind: EdgeKind::HasBlock,
                });
            }
            if let Some(term) = &bb.terminator {
                for succ in term.successors() {
                    let succ_symbol = format!("{path}::bb{}", succ.as_usize());
                    if let Some(dst_id) = symbol_to_id.get(&succ_symbol) {
                        edges.push(Edge {
                            src: bb_id,
                            dst: *dst_id,
                            kind: EdgeKind::Flow,
                        });
                    }
                }
                if let Some(unwind) = unwind_target(term) {
                    let unwind_symbol = format!("{path}::bb{}", unwind.as_usize());
                    if let Some(dst_id) = symbol_to_id.get(&unwind_symbol) {
                        edges.push(Edge {
                            src: bb_id,
                            dst: *dst_id,
                            kind: EdgeKind::Unwind,
                        });
                    }
                }
                if matches!(term.kind, mir::TerminatorKind::Return) {
                    if let Some(fn_id) = fn_id {
                        edges.push(Edge {
                            src: bb_id,
                            dst: fn_id,
                            kind: EdgeKind::Return,
                        });
                    }
                }
                if let mir::TerminatorKind::Call { func, .. } = &term.kind {
                    let call_symbol = format!("{path}::bb{idx}::call{call_index}");
                    if let Some(call_id) = symbol_to_id.get(&call_symbol) {
                        edges.push(Edge {
                            src: bb_id,
                            dst: *call_id,
                            kind: EdgeKind::HasBlock,
                        });
                    }
                    call_index += 1;
                    if let Some((callee_def_id, _)) = func.const_fn_def() {
                        let callee_symbol = format!("{}::fn", tcx.def_path_str(callee_def_id));
                        if let Some(dst_id) = symbol_to_id.get(&callee_symbol) {
                            if let Some(call_id) = symbol_to_id.get(&call_symbol) {
                                edges.push(Edge {
                                    src: *call_id,
                                    dst: *dst_id,
                                    kind: EdgeKind::Call,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(edges)
}

fn resolve_module_file_from_decl(decl_file: &str, mod_name: &str, path_attr: Option<&str>) -> Option<String> {
    let decl_path = Path::new(decl_file);
    let decl_path = if decl_path.is_absolute() {
        decl_path.to_path_buf()
    } else {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(decl_path));
        }
        if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
            let root_path = PathBuf::from(root);
            candidates.push(root_path.join(decl_path));
            // Also try workspace root ancestors for paths like "canon-utils/upg_analysis/src/lib.rs"
            for ancestor in root_path.ancestors().skip(1).take(4) {
                candidates.push(ancestor.join(decl_path));
            }
        }
        candidates
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| decl_path.to_path_buf())
    };
    let base_dir = decl_path.parent()?.to_path_buf();
    if let Some(path_lit) = path_attr {
        let path = if Path::new(path_lit).is_absolute() {
            PathBuf::from(path_lit)
        } else {
            base_dir.join(path_lit)
        };
        return Some(path.to_string_lossy().to_string());
    }
    let direct = base_dir.join(format!("{mod_name}.rs"));
    if direct.exists() {
        return Some(direct.to_string_lossy().to_string());
    }
    let nested = base_dir.join(mod_name).join("mod.rs");
    if nested.exists() {
        return Some(nested.to_string_lossy().to_string());
    }
    let prefix_mod = base_dir.join(format!("{mod_name}_mod.rs"));
    if prefix_mod.exists() {
        return Some(prefix_mod.to_string_lossy().to_string());
    }
    None
}

fn is_project_file(output_dir: &Path, raw: &str) -> bool {
    let Some(resolved) = resolve_source_path(raw) else {
        return false;
    };
    let Some(project_root) = output_dir.parent() else {
        return false;
    };
    let path = Path::new(&resolved);
    path.starts_with(project_root) && path.is_file()
}

fn module_path_attr_value(tcx: TyCtxt<'_>, def_id: DefId) -> Option<String> {
    for attr in tcx.get_attrs(def_id, sym::path) {
        if let Some(value) = attr.value_str() {
            return Some(value.to_string());
        }
    }
    None
}

fn span_info(tcx: TyCtxt<'_>, span: rustc_span::Span) -> (String, u32, u32, u32, u32) {
    let sm = tcx.sess.source_map();
    let loc_start = sm.lookup_char_pos(span.lo());
    let loc_end = sm.lookup_char_pos(span.hi());
    let raw_file = file_name_to_string(&loc_start.file.name);
    let file = resolve_source_path(&raw_file).unwrap_or(raw_file);
    let line = loc_start.line as u32;
    let col = loc_start.col.to_usize() as u32 + 1;
    let lo = byte_offset_for_loc(&file, line, col).unwrap_or(0) as u32;
    let hi = if file_name_to_string(&loc_end.file.name) == file {
        let end_line = loc_end.line as u32;
        let end_col = loc_end.col.to_usize() as u32 + 1;
        byte_offset_for_loc(&file, end_line, end_col).unwrap_or(lo as usize) as u32
    } else {
        lo
    };
    (file, line, col, lo, hi)
}

thread_local! {
    static FILE_CACHE: RefCell<HashMap<String, FileOffsets>> = RefCell::new(HashMap::new());
}

struct FileOffsets {
    text: String,
    line_starts: Vec<usize>,
}

fn byte_offset_for_loc(file: &str, line: u32, col: u32) -> Option<usize> {
    if line == 0 || col == 0 {
        return None;
    }
    let resolved = resolve_source_path(file)?;
    FILE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let entry = cache.entry(resolved.clone()).or_insert_with(|| {
            let text = std::fs::read_to_string(&resolved).unwrap_or_default();
            let mut line_starts = Vec::new();
            line_starts.push(0);
            for (idx, b) in text.bytes().enumerate() {
                if b == b'\n' {
                    line_starts.push(idx + 1);
                }
            }
            FileOffsets { text, line_starts }
        });

        let line_idx = (line - 1) as usize;
        if line_idx >= entry.line_starts.len() {
            return None;
        }
        let start = entry.line_starts[line_idx];
        let end = if line_idx + 1 < entry.line_starts.len() {
            entry.line_starts[line_idx + 1]
        } else {
            entry.text.len()
        };
        let slice = &entry.text[start..end];
        let mut byte_off = 0usize;
        let mut remaining = col.saturating_sub(1) as usize;
        for ch in slice.chars() {
            if remaining == 0 {
                break;
            }
            byte_off += ch.len_utf8();
            remaining -= 1;
        }
        Some(start + byte_off)
    })
}

fn resolve_source_path(raw: &str) -> Option<String> {
    let mut cleaned = raw.trim().to_string();
    if cleaned.starts_with('"') && cleaned.ends_with('"') && cleaned.len() >= 2 {
        cleaned = cleaned[1..cleaned.len() - 1].to_string();
    }
    if cleaned.is_empty() {
        return None;
    }
    let path = Path::new(&cleaned);
    if path.is_absolute() && path.exists() {
        return Some(path.to_string_lossy().to_string());
    }
    if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(root.clone()).join(path);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
        let root_path = PathBuf::from(root);
        for ancestor in root_path.ancestors().skip(1).take(4) {
            let candidate = ancestor.join(path);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(path);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn file_name_to_string(name: &rustc_span::FileName) -> String {
    use rustc_span::FileName;
    let raw = match name {
        FileName::Real(real) => real
            .local_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| name.prefer_local_unconditionally().to_string()),
        _ => name.prefer_local_unconditionally().to_string(),
    };
    let mut out = raw.replace('\n', " ").replace('\r', " ");
    if out.contains(',') {
        out = out.replace(',', ";");
    }
    out
}

fn local_def_ids(tcx: TyCtxt<'_>) -> Vec<DefId> {
    let crate_items = tcx.hir_crate_items(());
    let mut seen = std::collections::HashSet::new();
    let mut defs: Vec<DefId> = Vec::new();
    let root = CRATE_DEF_ID.to_def_id();
    if seen.insert(root) {
        defs.push(root);
    }
    for def_id in crate_items
        .definitions()
        .map(|id| id.to_def_id())
        .filter(|def_id| !tcx.is_automatically_derived(*def_id))
        .filter(|def_id| !tcx.is_synthetic_mir(*def_id))
    {
        if seen.insert(def_id) {
            defs.push(def_id);
        }
    }
    defs
}

fn param_nodes(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<NodeSpec> {
    let mut nodes = Vec::new();
    let Some(local) = def_id.as_local() else {
        return nodes;
    };
    let Some(body) = tcx.hir_maybe_body_owned_by(local) else {
        return nodes;
    };
    let params = body.params;
    for (idx, param) in params.iter().enumerate() {
        let (file, line, column, span_lo, span_hi) = span_info(tcx, param.span);
        nodes.push(NodeSpec {
            kind: NodeKind::Param,
            kind_id: 10,
            symbol: format!("{path}::param{idx}"),
            file,
            line,
            column,
            span_lo,
            span_hi,
        });
    }
    nodes
}

fn param_symbols(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<String> {
    let Some(local) = def_id.as_local() else {
        return Vec::new();
    };
    let Some(body) = tcx.hir_maybe_body_owned_by(local) else {
        return Vec::new();
    };
    let params = body.params;
    params
        .iter()
        .enumerate()
        .map(|(idx, _)| format!("{path}::param{idx}"))
        .collect()
}

fn impl_symbol(path: &str) -> String {
    format!("{path}::impl")
}

fn resolve_parent_module_id(
    tcx: TyCtxt<'_>,
    parent_path: &str,
    symbol_to_id: &BTreeMap<String, u32>,
) -> Option<u32> {
    if let Some(id) = symbol_to_id.get(parent_path) {
        return Some(*id);
    }
    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let mut candidates: Vec<String> = Vec::new();
    if parent_path == "crate" {
        candidates.push(crate_name.clone());
        candidates.push(String::new());
    } else if parent_path == crate_name {
        candidates.push("crate".to_string());
        candidates.push(String::new());
    } else if parent_path.is_empty() {
        candidates.push(crate_name.clone());
        candidates.push("crate".to_string());
    } else if !parent_path.contains("::") {
        candidates.push(crate_name.clone());
        candidates.push("crate".to_string());
        candidates.push(String::new());
    }
    for cand in candidates {
        if let Some(id) = symbol_to_id.get(&cand) {
            return Some(*id);
        }
    }
    None
}

fn resolve_parent_module_id_module(
    tcx: TyCtxt<'_>,
    parent_path: &str,
    symbol_to_id: &BTreeMap<String, u32>,
    id_to_kind: &HashMap<u32, NodeKind>,
) -> Option<u32> {
    let mut path = parent_path.to_string();
    loop {
        if let Some(id) = resolve_parent_module_id(tcx, &path, symbol_to_id) {
            if id_to_kind.get(&id) == Some(&NodeKind::Module) {
                return Some(id);
            }
        }
        if let Some((head, _)) = path.rsplit_once("::") {
            path = head.to_string();
            continue;
        }
        break;
    }
    resolve_parent_module_id(tcx, "crate", symbol_to_id)
        .filter(|id| id_to_kind.get(id) == Some(&NodeKind::Module))
}

fn add_module_import_edges(
    tcx: TyCtxt<'_>,
    nodes: &[Node],
    symbol_to_id: &BTreeMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    let mut seen = std::collections::BTreeSet::new();
    for edge in edges.iter() {
        if edge.kind == EdgeKind::Imports {
            seen.insert((edge.src, edge.dst));
        }
    }
    let modules: Vec<&Node> = nodes.iter().filter(|n| n.kind == NodeKind::Module).collect();
    for module in modules {
        let child_id = module.id;
        let sym = module.symbol.as_str();
        if sym.is_empty() {
            continue;
        }
        let parent_path = sym
            .rsplitn(2, "::")
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| "crate".to_string());
        if let Some(parent_id) = resolve_parent_module_id(tcx, &parent_path, symbol_to_id) {
            if parent_id == child_id {
                continue;
            }
            if seen.insert((parent_id, child_id)) {
                edges.push(Edge {
                    src: parent_id,
                    dst: child_id,
                    kind: EdgeKind::Imports,
                });
            }
        }
    }
}

fn ensure_fn_nodes_for_bb(
    mut nodes: Vec<Node>,
    mut spans_primary: Vec<SpanRange>,
    mut symbol_to_id: BTreeMap<String, u32>,
) -> (Vec<Node>, Vec<SpanRange>, BTreeMap<String, u32>) {
    let mut next_id = nodes.len() as u32;
    let mut pending: Vec<(String, usize)> = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        if node.kind != NodeKind::BasicBlock {
            continue;
        }
        let Some((base, _)) = node.symbol.split_once("::bb") else {
            continue;
        };
        let fn_symbol = format!("{base}::fn");
        if !symbol_to_id.contains_key(&fn_symbol) {
            pending.push((fn_symbol, idx));
        }
    }
    for (fn_symbol, bb_idx) in pending {
        let bb_node = &nodes[bb_idx];
        let id = next_id;
        next_id += 1;
        nodes.push(Node {
            id,
            kind: NodeKind::Function,
            symbol: fn_symbol.clone(),
            file: bb_node.file.clone(),
            line: bb_node.line,
            column: bb_node.column,
        });
        spans_primary.push(spans_primary[bb_idx]);
        symbol_to_id.insert(fn_symbol, id);
    }
    (nodes, spans_primary, symbol_to_id)
}

fn add_module_contains_edges_from_nodes(
    tcx: TyCtxt<'_>,
    nodes: &[Node],
    symbol_to_id: &BTreeMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    let mut local_map: BTreeMap<&str, u32> = BTreeMap::new();
    for node in nodes {
        local_map.insert(node.symbol.as_str(), node.id);
    }
    for node in nodes {
        let item_path = match node.kind {
            NodeKind::Function | NodeKind::Method => {
                node.symbol.strip_suffix("::fn").unwrap_or(&node.symbol)
            }
            NodeKind::Struct
            | NodeKind::Enum
            | NodeKind::Trait
            | NodeKind::Impl
            | NodeKind::Type => &node.symbol,
            _ => continue,
        };
        if item_path.is_empty() {
            continue;
        }
        let parent_path = item_path
            .rsplitn(2, "::")
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| "crate".to_string());
        let parent_id = resolve_parent_module_id(tcx, &parent_path, symbol_to_id)
            .or_else(|| local_map.get(format!("{parent_path}::fn").as_str()).copied());
        let parent_id = parent_id.or_else(|| resolve_parent_module_id(tcx, "crate", symbol_to_id));
        if let Some(parent_id) = parent_id {
            edges.push(Edge {
                src: parent_id,
                dst: node.id,
                kind: EdgeKind::Contains,
            });
        }
    }
}

fn add_callsite_block_edges(
    nodes: &[Node],
    symbol_to_id: &BTreeMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    for node in nodes {
        if node.kind != NodeKind::CallSite {
            continue;
        }
        let Some((bb_part, _call_part)) = node.symbol.rsplit_once("::call") else {
            continue;
        };
        let Some(bb_id) = symbol_to_id.get(bb_part).copied() else {
            continue;
        };
        edges.push(Edge {
            src: bb_id,
            dst: node.id,
            kind: EdgeKind::HasBlock,
        });
    }
}

fn add_call_edges_from_mir(
    tcx: TyCtxt<'_>,
    def_paths: &[(DefId, String)],
    symbol_to_id: &BTreeMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    for (def_id, path) in def_paths {
        let Some(local_def) = def_id.as_local() else { continue };
        if !tcx.is_mir_available(local_def) {
            continue;
        }
        let body = match tcx.hir_body_const_context(local_def) {
            Some(hir::ConstContext::ConstFn)
            | Some(hir::ConstContext::Const { .. })
            | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
            None => tcx.optimized_mir(local_def),
        };
        let mut call_index = 0usize;
        for (bb_idx, bb) in body.basic_blocks.iter().enumerate() {
            let Some(term) = &bb.terminator else { continue };
            let mir::TerminatorKind::Call { func, .. } = &term.kind else { continue };
            let Some(callee_def_id) = callee_def_id(tcx, func) else { continue };
            let callee_path = tcx.def_path_str(callee_def_id);
            let callee_symbol = format!("{callee_path}::fn");
            let Some(dst_id) = symbol_to_id.get(&callee_symbol).copied() else { continue };
            let callsite_symbol = format!("{path}::bb{bb_idx}::call{call_index}");
            let Some(src_id) = symbol_to_id.get(&callsite_symbol).copied() else { continue };
            edges.push(Edge {
                src: src_id,
                dst: dst_id,
                kind: EdgeKind::Call,
            });
            call_index += 1;
        }
    }
}

fn callee_def_id(_tcx: TyCtxt<'_>, func: &mir::Operand<'_>) -> Option<DefId> {
    match func {
        mir::Operand::Constant(c) => match c.ty().kind() {
            rustc_middle::ty::TyKind::FnDef(def_id, _) => Some(*def_id),
            _ => None,
        },
        _ => None,
    }
}

fn add_contains_for_isolated_types(
    tcx: TyCtxt<'_>,
    nodes: &[Node],
    symbol_to_id: &BTreeMap<String, u32>,
    edges: &mut Vec<Edge>,
) {
    let mut local_map: BTreeMap<&str, u32> = BTreeMap::new();
    for node in nodes {
        local_map.insert(node.symbol.as_str(), node.id);
    }
    let mut edge_src: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut edge_dst: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for edge in edges.iter() {
        edge_src.insert(edge.src);
        edge_dst.insert(edge.dst);
    }
    for node in nodes {
        if node.kind != NodeKind::Struct && node.kind != NodeKind::Enum {
            continue;
        }
        if edge_src.contains(&node.id) || edge_dst.contains(&node.id) {
            continue;
        }
        let item_path = node.symbol.as_str();
        if item_path.is_empty() {
            continue;
        }
        let parent_path = item_path
            .rsplitn(2, "::")
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| "crate".to_string());
        let parent_id = resolve_parent_module_id(tcx, &parent_path, symbol_to_id)
            .or_else(|| local_map.get(format!("{parent_path}::fn").as_str()).copied());
        if let Some(parent_id) = parent_id {
            edges.push(Edge {
                src: parent_id,
                dst: node.id,
                kind: EdgeKind::Contains,
            });
        }
    }
}

fn add_export_edges(
    tcx: TyCtxt<'_>,
    def_paths: &[(DefId, String)],
    symbol_to_id: &BTreeMap<String, u32>,
    id_to_kind: &HashMap<u32, NodeKind>,
    edges: &mut Vec<Edge>,
) {
    for (def_id, path) in def_paths {
        if !is_exported_item(tcx, *def_id) {
            continue;
        }
        let def_kind = tcx.def_kind(*def_id);
        if matches!(
            def_kind,
            rustc_hir::def::DefKind::Struct
                | rustc_hir::def::DefKind::Enum
                | rustc_hir::def::DefKind::Trait
                | rustc_hir::def::DefKind::Fn
                | rustc_hir::def::DefKind::AssocFn
                | rustc_hir::def::DefKind::Mod
                | rustc_hir::def::DefKind::Const
                | rustc_hir::def::DefKind::Static { .. }
                | rustc_hir::def::DefKind::TyAlias
        ) {
            let symbol = match def_kind {
                rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn => {
                    format!("{path}::fn")
                }
                _ => path.clone(),
            };
            let Some(item_id) = symbol_to_id.get(&symbol).copied() else { continue };
            let parent_path = path
                .rsplitn(2, "::")
                .nth(1)
                .map(str::to_string)
                .unwrap_or_else(|| "crate".to_string());
            if let Some(parent_id) =
                resolve_parent_module_id_module(tcx, &parent_path, symbol_to_id, id_to_kind)
            {
                edges.push(Edge {
                    src: parent_id,
                    dst: item_id,
                    kind: EdgeKind::Export,
                });
            }
        }
    }
}

fn add_public_use_edges(
    tcx: TyCtxt<'_>,
    symbol_to_id: &BTreeMap<String, u32>,
    id_to_kind: &HashMap<u32, NodeKind>,
    edges: &mut Vec<Edge>,
) {
    use rustc_hir::def::Res;
    use rustc_hir::intravisit::{self, Visitor};
    use rustc_middle::ty::Visibility;

    struct PublicUseVisitor<'a, 'tcx> {
        tcx: TyCtxt<'tcx>,
        parent_id: u32,
        symbol_to_id: &'a BTreeMap<String, u32>,
        edges: &'a mut Vec<Edge>,
    }

    impl<'a, 'tcx> Visitor<'tcx> for PublicUseVisitor<'a, 'tcx> {
        fn visit_path(&mut self, path: &rustc_hir::Path<'tcx>, _id: rustc_hir::HirId) {
            if let Res::Def(_, def_id) = path.res {
                let def_kind = self.tcx.def_kind(def_id);
                let symbol = match def_kind {
                    rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn => {
                        format!("{}::fn", self.tcx.def_path_str(def_id))
                    }
                    _ => self.tcx.def_path_str(def_id),
                };
                if let Some(item_id) = self.symbol_to_id.get(&symbol).copied() {
                    self.edges.push(Edge {
                        src: self.parent_id,
                        dst: item_id,
                        kind: EdgeKind::PublicUse,
                    });
                }
            }
            intravisit::walk_path(self, path);
        }
    }

    let crate_items = tcx.hir_crate_items(());
    for def_id in crate_items
        .definitions()
        .map(|id| id.to_def_id())
        .filter(|def_id| !tcx.is_automatically_derived(*def_id))
        .filter(|def_id| !tcx.is_synthetic_mir(*def_id))
    {
        let Some(local) = def_id.as_local() else {
            continue;
        };
        let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) else {
            continue;
        };
        if !matches!(item.kind, rustc_hir::ItemKind::Use(_, _)) {
            continue;
        }
        if !matches!(tcx.visibility(def_id), Visibility::Public | Visibility::Restricted(_)) {
            continue;
        }
        let Some(parent_def_id) = tcx.opt_parent(def_id) else {
            continue;
        };
        let parent_path = tcx.def_path_str(parent_def_id);
        let Some(parent_id) =
            resolve_parent_module_id_module(tcx, &parent_path, symbol_to_id, id_to_kind)
        else {
            continue;
        };
        let mut visitor = PublicUseVisitor {
            tcx,
            parent_id,
            symbol_to_id,
            edges,
        };
        visitor.visit_item(item);
    }
}

fn is_exported_item(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    use rustc_middle::ty::Visibility;
    matches!(
        tcx.visibility(def_id),
        Visibility::Public | Visibility::Restricted(_)
    )
}

fn field_nodes(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<NodeSpec> {
    let mut nodes = Vec::new();
    for field in field_symbols(tcx, def_id, path) {
        let span = tcx.def_span(def_id);
        let (file, line, column, span_lo, span_hi) = span_info(tcx, span);
        nodes.push(NodeSpec {
            kind: NodeKind::Field,
            kind_id: 9,
            symbol: field,
            file,
            line,
            column,
            span_lo,
            span_hi,
        });
    }
    nodes
}

fn field_symbols(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let adt_def = tcx.adt_def(def_id);
    for variant in adt_def.variants() {
        for field in variant.fields.iter() {
            let name = field.name.to_string();
            out.push(format!("{path}::{}", name));
        }
    }
    out
}

fn unwind_target(term: &mir::Terminator<'_>) -> Option<mir::BasicBlock> {
    match term.kind.unwind() {
        Some(mir::UnwindAction::Cleanup(bb)) => Some(*bb),
        Some(mir::UnwindAction::Terminate(_))
        | Some(mir::UnwindAction::Unreachable)
        | Some(mir::UnwindAction::Continue) => None,
        None => None,
    }
}
