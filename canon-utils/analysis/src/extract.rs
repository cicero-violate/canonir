use crate::csr::build_csr;
use crate::emit::OutputConfig;
use crate::types::{Edge, EdgeKind, Metadata, Node, NodeKind};
use anyhow::Result;
use rustc_hir as hir;
use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::Pos;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct UpgGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub csr: crate::csr::CsrGraph,
    pub metadata: Metadata,
}

#[derive(Debug, Clone)]
struct NodeSpec {
    kind: NodeKind,
    kind_id: u32,
    symbol: String,
    file: String,
    line: u32,
    column: u32,
}

pub fn extract_and_write(tcx: TyCtxt<'_>, config: &OutputConfig) -> Result<UpgGraph> {
    let graph = extract_upg(tcx, &config.output_dir)?;
    let merged = merge_with_existing(&config.output_dir, graph)?;
    crate::emit::write_outputs(&merged, &config.output_dir)?;
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

    node_specs.sort_by(|a, b| a.symbol.cmp(&b.symbol).then_with(|| a.kind_id.cmp(&b.kind_id)));
    let mut nodes: Vec<Node> = Vec::with_capacity(node_specs.len());
    let mut symbol_to_id: BTreeMap<String, u32> = BTreeMap::new();
    for (idx, spec) in node_specs.into_iter().enumerate() {
        let id = idx as u32;
        symbol_to_id.insert(spec.symbol.clone(), id);
        nodes.push(Node {
            id,
            kind: spec.kind,
            symbol: spec.symbol,
            file: spec.file,
            line: spec.line,
            column: spec.column,
        });
    }

    let edges = build_edges(tcx, &def_paths, &symbol_to_id)?;
    let csr = build_csr(nodes.len() as u32, &edges);
    let metadata = Metadata {
        project,
        node_count: nodes.len() as u32,
        edge_count: csr.col_idx.len() as u32,
        generated_by: "UPG extractor".to_string(),
    };

    Ok(UpgGraph {
        nodes,
        edges,
        csr,
        metadata,
    })
}

fn merge_with_existing(output_dir: &Path, graph: UpgGraph) -> Result<UpgGraph> {
    let nodes_path = output_dir.join("nodes.csv");
    let edges_path = output_dir.join("edges.csv");
    if !nodes_path.exists() || !edges_path.exists() {
        return Ok(graph);
    }
    let mut nodes = read_nodes_csv(nodes_path)?;
    let mut edges = read_edges_csv(edges_path)?;
    let offset = nodes.len() as u32;
    let mut new_nodes = graph.nodes;
    for node in &mut new_nodes {
        node.id += offset;
    }
    let mut new_edges = graph.edges;
    for edge in &mut new_edges {
        edge.src += offset;
        edge.dst += offset;
    }
    nodes.extend(new_nodes);
    edges.extend(new_edges);
    let csr = build_csr(nodes.len() as u32, &edges);
    let metadata = Metadata {
        project: graph.metadata.project,
        node_count: nodes.len() as u32,
        edge_count: csr.col_idx.len() as u32,
        generated_by: graph.metadata.generated_by,
    };
    Ok(UpgGraph {
        nodes,
        edges,
        csr,
        metadata,
    })
}

fn read_nodes_csv(path: std::path::PathBuf) -> Result<Vec<Node>> {
    let content = std::fs::read_to_string(path)?;
    let mut nodes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 6 {
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
        "HAS_FIELD" => Ok(EdgeKind::HasField),
        "HAS_METHOD" => Ok(EdgeKind::HasMethod),
        "HAS_BLOCK" => Ok(EdgeKind::HasBlock),
        "HAS_PARAM" => Ok(EdgeKind::HasParam),
        "IMPORTS" => Ok(EdgeKind::Imports),
        "FLOW" => Ok(EdgeKind::Flow),
        "CALL" => Ok(EdgeKind::Call),
        "RETURN" => Ok(EdgeKind::Return),
        "UNWIND" => Ok(EdgeKind::Unwind),
        "IMPLEMENTS" => Ok(EdgeKind::Implements),
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
    let (file, line, column) = span_info(tcx, span);

    match tcx.def_kind(def_id) {
        rustc_hir::def::DefKind::Fn => {
            nodes.push(NodeSpec {
                kind: NodeKind::Function,
                kind_id: 1,
                symbol: format!("{path}::fn"),
                file,
                line,
                column,
            });
            nodes.extend(param_nodes(tcx, def_id, path));
        }
        rustc_hir::def::DefKind::AssocFn => {
            nodes.push(NodeSpec {
                kind: NodeKind::Method,
                kind_id: 2,
                symbol: format!("{path}::fn"),
                file,
                line,
                column,
            });
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
            });
        }
        rustc_hir::def::DefKind::Impl { .. } => {
            nodes.push(NodeSpec {
                kind: NodeKind::Impl,
                kind_id: 6,
                symbol: path.to_string(),
                file,
                line,
                column,
            });
        }
        rustc_hir::def::DefKind::Mod => {
            nodes.push(NodeSpec {
                kind: NodeKind::Module,
                kind_id: 7,
                symbol: path.to_string(),
                file,
                line,
                column,
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
    let mut nodes = Vec::new();
    for (idx, bb) in body.basic_blocks.iter().enumerate() {
        let span = bb.terminator.as_ref().map(|t| t.source_info.span).unwrap_or(body.span);
        let (file, line, column) = span_info(tcx, span);
        nodes.push(NodeSpec {
            kind: NodeKind::BasicBlock,
            kind_id: 20,
            symbol: format!("{path}::bb{idx}"),
            file,
            line,
            column,
        });
    }

    let mut call_index = 0usize;
    for (idx, bb) in body.basic_blocks.iter().enumerate() {
        if let Some(term) = &bb.terminator {
            if matches!(term.kind, mir::TerminatorKind::Call { .. }) {
                let span = term.source_info.span;
                let (file, line, column) = span_info(tcx, span);
                nodes.push(NodeSpec {
                    kind: NodeKind::CallSite,
                    kind_id: 21,
                    symbol: format!("{path}::bb{idx}::call{call_index}"),
                    file,
                    line,
                    column,
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

    for (def_id, path) in def_paths {
        match tcx.def_kind(*def_id) {
            rustc_hir::def::DefKind::Struct | rustc_hir::def::DefKind::Enum => {
                let parent_id = symbol_to_id.get(path).copied();
                if let Some(parent_id) = parent_id {
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
            rustc_hir::def::DefKind::Impl { of_trait } => {
                let impl_id = symbol_to_id.get(path).copied();
                if let Some(impl_id) = impl_id {
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
                }
            }
            rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn => {
                let fn_symbol = format!("{path}::fn");
                let fn_id = symbol_to_id.get(&fn_symbol).copied();
                if let Some(fn_id) = fn_id {
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
                    if let Some((callee_def_id, _)) = func.const_fn_def() {
                        let callee_symbol = format!("{}::fn", tcx.def_path_str(callee_def_id));
                        if let Some(dst_id) = symbol_to_id.get(&callee_symbol) {
                            edges.push(Edge {
                                src: bb_id,
                                dst: *dst_id,
                                kind: EdgeKind::Call,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(edges)
}

fn span_info(tcx: TyCtxt<'_>, span: rustc_span::Span) -> (String, u32, u32) {
    let sm = tcx.sess.source_map();
    let loc = sm.lookup_char_pos(span.lo());
    let mut file = format!("{:?}", loc.file.name);
    if file.contains("name: \"") {
        if let Some(start) = file.find("name: \"") {
            let rest = &file[start + 7..];
            if let Some(end) = rest.find('"') {
                file = rest[..end].to_string();
            }
        }
    }
    if file.contains('\n') || file.contains('\r') {
        file = file.replace('\n', " ").replace('\r', " ");
    }
    if file.contains(',') {
        file = file.replace(',', ";");
    }
    let line = loc.line as u32;
    let col = loc.col.to_usize() as u32 + 1;
    (file, line, col)
}

fn local_def_ids(tcx: TyCtxt<'_>) -> Vec<DefId> {
    let crate_items = tcx.hir_crate_items(());
    crate_items
        .definitions()
        .map(|id| id.to_def_id())
        .filter(|def_id| !tcx.is_automatically_derived(*def_id))
        .filter(|def_id| !tcx.is_synthetic_mir(*def_id))
        .collect()
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
        let (file, line, column) = span_info(tcx, param.span);
        nodes.push(NodeSpec {
            kind: NodeKind::Param,
            kind_id: 10,
            symbol: format!("{path}::param{idx}"),
            file,
            line,
            column,
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

fn field_nodes(tcx: TyCtxt<'_>, def_id: DefId, path: &str) -> Vec<NodeSpec> {
    let mut nodes = Vec::new();
    for field in field_symbols(tcx, def_id, path) {
        let span = tcx.def_span(def_id);
        let (file, line, column) = span_info(tcx, span);
        nodes.push(NodeSpec {
            kind: NodeKind::Field,
            kind_id: 9,
            symbol: field,
            file,
            line,
            column,
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
