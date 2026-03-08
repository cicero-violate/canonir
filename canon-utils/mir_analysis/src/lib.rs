#![feature(rustc_private)]

extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use anyhow::{anyhow, Result};
use rustc_hir as hir;
use rustc_middle::mir::{self};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::{DefId, LOCAL_CRATE};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum NodeKind {
    Function,
    BasicBlock,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EdgeKind {
    ControlFlow,
    Call,
    Unwind,
    Return,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub label: String,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub src: u32,
    pub dst: u32,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub crate_name: String,
    pub generated_at_unix: u64,
    pub node_count: u32,
    pub edge_count: u32,
    pub edge_kind_counts: BTreeMap<String, u32>,
}

#[derive(Debug, Clone)]
pub struct MirGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub csr: CsrGraph,
    pub metadata: Metadata,
}

#[derive(Debug, Clone)]
pub struct CsrGraph {
    pub row_ptr: Vec<u32>,
    pub col_idx: Vec<u32>,
}

impl CsrGraph {
    pub fn neighbors(&self, node: u32) -> &[u32] {
        let start_u32 = *self.row_ptr.get(node as usize).unwrap_or(&0);
        let end_u32 = self
            .row_ptr
            .get(node as usize + 1)
            .copied()
            .unwrap_or(start_u32);
        let start = start_u32 as usize;
        let end = end_u32 as usize;
        &self.col_idx[start..end]
    }
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub output_dir: PathBuf,
    pub include_fn_nodes: bool,
}

pub fn extract_and_write(tcx: TyCtxt<'_>, config: &OutputConfig) -> Result<MirGraph> {
    let graph = extract_mir_graph(tcx, config.include_fn_nodes)?;
    write_graph_outputs(&graph, &config.output_dir)?;
    Ok(graph)
}

pub fn extract_mir_graph(tcx: TyCtxt<'_>, include_fn_nodes: bool) -> Result<MirGraph> {
    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let mut node_specs: Vec<(String, NodeKind)> = Vec::new();
    let mut def_paths: Vec<(DefId, String)> = Vec::new();

    for def_id in local_mir_def_ids(tcx) {
        let path = tcx.def_path_str(def_id);
        def_paths.push((def_id, path.clone()));
        if include_fn_nodes {
            node_specs.push((format!("{path}::fn"), NodeKind::Function));
        }
        let body = load_mir_body(tcx, def_id)?;
        let bb_count = body.basic_blocks.len();
        node_specs.extend((0..bb_count).map(|idx| (format!("{path}::bb{idx}"), NodeKind::BasicBlock)));
    }

    node_specs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut nodes: Vec<Node> = Vec::with_capacity(node_specs.len());
    let mut label_to_id: BTreeMap<String, u32> = BTreeMap::new();
    for (id, (label, kind)) in node_specs.into_iter().enumerate() {
        let node_id = id as u32;
        label_to_id.insert(label.clone(), node_id);
        nodes.push(Node { id: node_id, label, kind });
    }

    let mut edges: Vec<Edge> = Vec::new();
    for (def_id, path) in def_paths {
        let body = load_mir_body(tcx, def_id)?;
        let fn_label = format!("{path}::fn");
        let fn_node = include_fn_nodes.then(|| label_to_id.get(&fn_label).copied()).flatten();
        for (bb_idx, bb_data) in body.basic_blocks.iter().enumerate() {
            let src_label = format!("{path}::bb{bb_idx}");
            let src_id = match label_to_id.get(&src_label) {
                Some(id) => *id,
                None => continue,
            };

            if let Some(term) = &bb_data.terminator {
                for succ in term.successors() {
                    let dst_label = format!("{path}::bb{}", succ.as_usize());
                    if let Some(dst_id) = label_to_id.get(&dst_label) {
                        edges.push(Edge { src: src_id, dst: *dst_id, kind: EdgeKind::ControlFlow });
                    }
                }

                if let Some(unwind) = unwind_target(term) {
                    let dst_label = format!("{path}::bb{}", unwind.as_usize());
                    if let Some(dst_id) = label_to_id.get(&dst_label) {
                        edges.push(Edge { src: src_id, dst: *dst_id, kind: EdgeKind::Unwind });
                    }
                }

                if matches!(term.kind, mir::TerminatorKind::Return) {
                    if let Some(dst_id) = fn_node {
                        edges.push(Edge { src: src_id, dst: dst_id, kind: EdgeKind::Return });
                    }
                }

                if let mir::TerminatorKind::Call { func, .. } = &term.kind {
                    if let Some((callee_def_id, _)) = func.const_fn_def() {
                        let callee_label = format!("{}::fn", tcx.def_path_str(callee_def_id));
                        if let Some(dst_id) = label_to_id.get(&callee_label) {
                            edges.push(Edge { src: src_id, dst: *dst_id, kind: EdgeKind::Call });
                        }
                    }
                }
            }
        }
    }

    let csr = build_csr(nodes.len() as u32, &edges);
    let metadata = build_metadata(&crate_name, &nodes, &edges, csr.col_idx.len() as u32);

    Ok(MirGraph {
        nodes,
        edges,
        csr,
        metadata,
    })
}

pub fn build_csr(node_count: u32, edges: &[Edge]) -> CsrGraph {
    let mut unique: BTreeSet<(u32, u32)> = BTreeSet::new();
    for edge in edges {
        unique.insert((edge.src, edge.dst));
    }

    let mut row_ptr = vec![0u32; node_count as usize + 1];
    let mut col_idx: Vec<u32> = Vec::with_capacity(unique.len());
    let mut cursor = 0usize;

    for node in 0..node_count {
        row_ptr[node as usize] = cursor as u32;
        for &(src, dst) in unique.range((node, 0)..=(node, u32::MAX)) {
            if src != node {
                break;
            }
            col_idx.push(dst);
            cursor += 1;
        }
    }
    row_ptr[node_count as usize] = cursor as u32;

    CsrGraph { row_ptr, col_idx }
}

pub fn write_graph_outputs(graph: &MirGraph, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;
    write_nodes_txt(output_dir, &graph.nodes)?;
    write_edges_txt(output_dir, &graph.nodes, &graph.edges)?;
    write_bin_u32(output_dir.join("csr_row_ptr.bin"), &graph.csr.row_ptr)?;
    write_bin_u32(output_dir.join("csr_col_idx.bin"), &graph.csr.col_idx)?;
    let metadata_path = output_dir.join("metadata.json");
    let file = fs::File::create(metadata_path)?;
    serde_json::to_writer_pretty(file, &graph.metadata)
        .map_err(|err| anyhow!("failed to write metadata.json: {err}"))
}

pub fn load_csr(output_dir: &Path) -> Result<CsrGraph> {
    let row_ptr = read_bin_u32(output_dir.join("csr_row_ptr.bin"))?;
    let col_idx = read_bin_u32(output_dir.join("csr_col_idx.bin"))?;
    Ok(CsrGraph { row_ptr, col_idx })
}

pub fn find_path(csr: &CsrGraph, start: u32, goal: u32) -> Option<Vec<u32>> {
    let node_count = csr.row_ptr.len().saturating_sub(1);
    if start as usize >= node_count || goal as usize >= node_count {
        return None;
    }

    let mut prev: Vec<Option<u32>> = vec![None; node_count];
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(start);
    prev[start as usize] = Some(start);

    while let Some(node) = queue.pop_front() {
        if node == goal {
            return Some(reconstruct_path(&prev, start, goal));
        }
        for &next in csr.neighbors(node) {
            if prev[next as usize].is_none() {
                prev[next as usize] = Some(node);
                queue.push_back(next);
            }
        }
    }
    None
}

fn reconstruct_path(prev: &[Option<u32>], start: u32, goal: u32) -> Vec<u32> {
    let mut path: Vec<u32> = Vec::new();
    let mut cursor = goal;
    loop {
        path.push(cursor);
        if cursor == start {
            break;
        }
        cursor = match prev[cursor as usize] {
            Some(p) => p,
            None => break,
        };
    }
    path.reverse();
    path
}

fn build_metadata(crate_name: &str, nodes: &[Node], edges: &[Edge], edge_count: u32) -> Metadata {
    let mut edge_kind_counts: BTreeMap<String, u32> = BTreeMap::new();
    for edge in edges {
        let key = format!("{:?}", edge.kind);
        *edge_kind_counts.entry(key).or_insert(0) += 1;
    }
    Metadata {
        crate_name: crate_name.to_string(),
        generated_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        node_count: nodes.len() as u32,
        edge_count,
        edge_kind_counts,
    }
}

fn local_mir_def_ids(tcx: TyCtxt<'_>) -> Vec<DefId> {
    let crate_items = tcx.hir_crate_items(());
    crate_items
        .definitions()
        .map(|id| id.to_def_id())
        .filter(|def_id| !tcx.is_automatically_derived(*def_id))
        .filter(|def_id| !tcx.is_synthetic_mir(*def_id))
        .filter(|def_id| def_id.as_local().is_some_and(|local| tcx.is_mir_available(local)))
        .collect()
}

fn load_mir_body<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> Result<&'tcx mir::Body<'tcx>> {
    let Some(local_def) = def_id.as_local() else {
        return Err(anyhow!("expected local DefId for MIR body"));
    };
    let body = match tcx.hir_body_const_context(local_def) {
        Some(hir::ConstContext::ConstFn)
        | Some(hir::ConstContext::Const { .. })
        | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };
    Ok(body)
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

fn write_nodes_txt(output_dir: &Path, nodes: &[Node]) -> Result<()> {
    let path = output_dir.join("nodes.txt");
    let mut file = fs::File::create(path)?;
    for node in nodes {
        writeln!(file, "{} {}", node.id, node.label)?;
    }
    Ok(())
}

fn write_edges_txt(output_dir: &Path, nodes: &[Node], edges: &[Edge]) -> Result<()> {
    let path = output_dir.join("edges.txt");
    let mut file = fs::File::create(path)?;
    let id_to_label: BTreeMap<u32, &str> = nodes.iter().map(|n| (n.id, n.label.as_str())).collect();
    let mut unique: BTreeSet<(u32, u32)> = BTreeSet::new();
    for edge in edges {
        unique.insert((edge.src, edge.dst));
    }
    for (src, dst) in unique {
        let src_label = id_to_label.get(&src).unwrap_or(&"");
        let dst_label = id_to_label.get(&dst).unwrap_or(&"");
        writeln!(file, "{src_label} --> {dst_label}")?;
    }
    Ok(())
}

fn write_bin_u32(path: PathBuf, values: &[u32]) -> Result<()> {
    let mut file = fs::File::create(path)?;
    for &value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn read_bin_u32(path: PathBuf) -> Result<Vec<u32>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("invalid u32 binary length"));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}
