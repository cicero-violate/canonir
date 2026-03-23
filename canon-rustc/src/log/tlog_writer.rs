use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use canon_ir::edge::EdgeKind as CanonEdgeKind;
use canon_ir::ir::CanonIR;
use canon_ir::node::{CanonId, CanonNodeKind};
use crate::types::{EdgeKind, NodeKind};
use crate::capture::{SpanInfo, SymbolSpanBundle};
use serde_json::json;
use crate::invariants;
use canon_event::canon_emit;
use crate::event_stream::event::RustcEvent;

pub struct TlogWriter {
    tlog_path: std::path::PathBuf,
    session_start_offset: u64,
}

impl TlogWriter {
    pub fn open(path: &Path) -> Result<Self> {
        // Resolve the segment directory so session_start_offset reflects
        // the binary tlog position (used by legacy index writer).
        let dir = if path.is_dir() || path.to_string_lossy().ends_with(".tlog.d") {
            path.to_path_buf()
        } else {
            path.with_extension("tlog.d")
        };
        let session_start_offset = dir
            .read_dir()
            .ok()
            .and_then(|mut d| d.next())
            .map(|_| 0u64)
            .unwrap_or(0);
        Ok(Self {
            tlog_path: dir,
            session_start_offset,
        })
    }

    pub fn write_session(&mut self, project: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::SessionStart(canon_event::SessionStart {
            project: project.to_string(), schema: 2, byte_offset: self.session_start_offset,
        }))?, &self.tlog_path)?;
        canon_emit!("rustc", "crate_compiled", json!({
            "crate": project, "schema": 2, "byte_offset": self.session_start_offset
        }), &self.tlog_path)
    }

    pub fn write_node(&mut self, sym: &str, kind: &str, file: &str, line: u32, col: u32, lo: u32, hi: u32) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::NodeDefined(canon_event::NodeDefined {
            symbol: sym.to_string(), kind: kind.to_string(), file: file.to_string(),
            line, col, lo, hi,
        }))?, &self.tlog_path)?;
        canon_emit!("rustc", "symbol_emitted", json!({
            "sym": sym, "kind": kind, "file": file, "line": line, "col": col, "lo": lo, "hi": hi
        }), &self.tlog_path)
    }

    pub fn write_edge(&mut self, src: &str, dst: &str, kind: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::EdgeDefined(canon_event::EdgeDefined {
            src: src.to_string(), dst: dst.to_string(), kind: kind.to_string(),
        }))?, &self.tlog_path)?;
        canon_emit!("rustc", "dependency_edge", json!({
            "src": src, "dst": dst, "kind": kind
        }), &self.tlog_path)
    }

    pub fn write_callsite(&mut self, kind: &str, resolved: bool) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::CallsiteObserved(canon_event::CallsiteObserved {
            kind: kind.to_string(), resolved,
        }))?, &self.tlog_path)
    }

    pub fn write_file(&mut self, path: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::FileSeen(canon_event::FileSeen {
            path: path.to_string(),
        }))?, &self.tlog_path)?;
        canon_emit!("rustc", "file_processed", json!({ "path": path }), &self.tlog_path)
    }

    pub fn write_symbol(&mut self, sym: &str, kind: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::SymbolDefined(canon_event::SymbolDefined {
            symbol: sym.to_string(), kind: kind.to_string(),
        }))?, &self.tlog_path)
    }

    pub fn write_span(&mut self, sym: &str, span: &SpanInfo) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::SpanDefined(canon_event::SpanDefined {
            symbol: sym.to_string(), file: span.file.clone(),
            line: span.line, col: span.col, lo: span.lo, hi: span.hi,
        }))?, &self.tlog_path)
    }

    pub fn write_warning(&mut self, msg: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::WarningCaptured(canon_event::WarningCaptured {
            message: msg.to_string(),
        }))?, &self.tlog_path)
    }

    pub fn write_node_remove(&mut self, sym: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::NodeRemoved(canon_event::NodeRemoved {
            symbol: sym.to_string(),
        }))?, &self.tlog_path)
    }

    pub fn write_edge_remove(&mut self, src: &str, dst: &str, kind: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::EdgeRemoved(canon_event::EdgeRemoved {
            src: src.to_string(), dst: dst.to_string(), kind: kind.to_string(),
        }))?, &self.tlog_path)
    }

    pub fn write_compilation_unit_finished(&mut self, crate_name: &str) -> Result<()> {
        canon_emit!("rustc", "rustc_event", serde_json::to_value(RustcEvent::CompilationUnitFinished(canon_event::CompilationUnitFinished {
            crate_name: crate_name.to_string(),
        }))?, &self.tlog_path)?;
        canon_emit!("rustc", "crate_compiled", json!({ "crate": crate_name }), &self.tlog_path)
    }

    pub fn write_panic(&mut self, record: &serde_json::Value) -> Result<()> {
        let event: RustcEvent = serde_json::from_value(record.clone())?;
        canon_emit!("rustc", "rustc_event", serde_json::to_value(event)?, &self.tlog_path)
    }

    pub fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn session_start_offset(&self) -> u64 {
        self.session_start_offset
    }
}


pub fn emit_ir_tlog(
    ir: &CanonIR,
    path: &Path,
    _project: &str,
    span_bundle: Option<&SymbolSpanBundle>,
) -> Result<()> {
    invariants::validate_structural(ir)?;
    let mut writer = TlogWriter::open(path)?;

    let labels = build_unique_labels(ir);
    let mut files: HashSet<String> = HashSet::new();

    let span_lookup = span_bundle.map(build_span_lookup);
    let mut span_cursor: HashMap<String, usize> = HashMap::new();
    for node in &ir.nodes {
        let label = label_for(&labels, node.id);
        let kind = canon_node_kind(&node.kind);
        let mut file = node_file(ir, &node.kind);
        let mut line = 0u32;
        let mut col = 0u32;
        let mut lo = 0u32;
        let mut hi = 0u32;

        if let Some(map) = span_lookup.as_ref() {
            for key in node_span_keys(ir, &node.kind, label) {
                if let Some(list) = map.get(&key) {
                    let idx = span_cursor.entry(key.clone()).or_insert(0);
                    if *idx < list.len() {
                        let span = &list[*idx];
                        *idx += 1;
                        if file.as_deref().unwrap_or("").is_empty() {
                            file = Some(span.file.clone());
                        }
                        line = span.line;
                        col = span.col;
                        lo = span.lo;
                        hi = span.hi;
                        break;
                    }
                }
            }
        }
        if let Some(file_path) = file.as_ref() {
            if !file_path.is_empty() {
                files.insert(file_path.clone());
            }
        }
        writer.write_node(
            label,
            node_kind_str(kind),
            file.as_deref().unwrap_or(""),
            line,
            col,
            lo,
            hi,
        )?;
    }

    emit_block_and_callsite_edges(&mut writer, &labels, ir)?;
    emit_cfg_edges_from_bodies(&mut writer, &labels, ir)?;

    emit_ir_edges(&mut writer, &labels, &ir.name_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.type_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.module_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.cfg_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.call_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.region_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.value_graph)?;
    emit_ir_edges(&mut writer, &labels, &ir.macro_graph)?;

    if let Some(bundle) = span_bundle {
        emit_symbol_kinds(&mut writer, bundle)?;
        emit_symbol_spans(&mut writer, bundle)?;
    }

    for _ in 0..count_ir_call_edges(&ir.call_graph) {
        writer.write_callsite("CALL", true)?;
    }

    for file in files {
        writer.write_file(&file)?;
    }

    writer.flush()?;
    Ok(())
}

fn build_unique_labels(ir: &CanonIR) -> Vec<String> {
    let mut base: Vec<String> = Vec::with_capacity(ir.nodes.len());
    for node in &ir.nodes {
        let label = node_base_label(ir, &node.kind);
        base.push(label);
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    for label in &base {
        let key = if label.is_empty() { "<empty>".to_string() } else { label.clone() };
        *counts.entry(key).or_insert(0) += 1;
    }

    base.into_iter()
        .enumerate()
        .map(|(idx, label)| {
            let key = if label.is_empty() { "<empty>".to_string() } else { label.clone() };
            let count = counts.get(&key).copied().unwrap_or(1);
            if label.is_empty() {
                format!("node#{}", idx)
            } else if count > 1 {
                format!("{label}#{}", idx)
            } else {
                label
            }
        })
        .collect()
}

fn label_for(labels: &[String], id: CanonId) -> &str {
    labels.get(id.0 as usize).map(|s| s.as_str()).unwrap_or("")
}

fn node_base_label(ir: &CanonIR, kind: &CanonNodeKind) -> String {
    match kind {
        CanonNodeKind::Crate { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Module { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::Struct { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Enum { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Trait { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::AssocType { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::AssocConst { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Impl { .. } => "impl".to_string(),
        CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::FnSig { .. } => "fn_sig".to_string(),
        CanonNodeKind::Type { .. } => "type".to_string(),
        CanonNodeKind::Field { name_id, .. } => name_id.map(|id| ir.lookup_name(id).to_string()).unwrap_or_else(|| "field".to_string()),
        CanonNodeKind::Param { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::GenericParam { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::WherePred { .. } => "where_pred".to_string(),
        CanonNodeKind::Variant { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Attr { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::Lifetime { name_id } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Const { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Static { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::Use { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::ExternCrate { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::TypeAlias { name_id, .. } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::TypeRef { name_id } => ir.lookup_name(*name_id).to_string(),
        CanonNodeKind::MacroCall { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::PathRef { path_id } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::Body { .. } => "body".to_string(),
        CanonNodeKind::BasicBlock { .. } => "bb".to_string(),
        CanonNodeKind::MatchArm { .. } => "match_arm".to_string(),
        CanonNodeKind::Pattern { .. } => "pattern".to_string(),
        CanonNodeKind::VisPath { path_id, .. } => ir.lookup_path(*path_id).to_string(),
        CanonNodeKind::Local { name_id, .. } => ir.lookup_name(*name_id).to_string(),
    }
}

fn node_file(ir: &CanonIR, kind: &CanonNodeKind) -> Option<String> {
    let path = match kind {
        CanonNodeKind::Module { path_id, .. } => ir.lookup_path(*path_id),
        CanonNodeKind::Use { path_id, .. } => ir.lookup_path(*path_id),
        CanonNodeKind::VisPath { path_id, .. } => ir.lookup_path(*path_id),
        CanonNodeKind::MacroCall { path_id, .. } => ir.lookup_path(*path_id),
        CanonNodeKind::PathRef { path_id } => ir.lookup_path(*path_id),
        _ => return None,
    };
    let looks_like_file = (path.contains('/') || path.contains('\\')) && path.ends_with(".rs");
    if looks_like_file {
        Some(path.to_string())
    } else {
        None
    }
}

fn node_span_keys(ir: &CanonIR, kind: &CanonNodeKind, label: &str) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    if let Some(path) = node_span_path(ir, kind) {
        keys.push(path.to_string());
    }
    let base = label.rsplit_once('#').map(|(b, _)| b).unwrap_or(label);
    if !base.is_empty() && !keys.iter().any(|k| k == base) {
        keys.push(base.to_string());
    }
    keys
}

fn node_span_path<'a>(ir: &'a CanonIR, kind: &CanonNodeKind) -> Option<&'a str> {
    match kind {
        CanonNodeKind::Module { path_id, .. } => Some(ir.lookup_path(*path_id)),
        CanonNodeKind::Use { path_id, .. } => Some(ir.lookup_path(*path_id)),
        CanonNodeKind::VisPath { path_id, .. } => Some(ir.lookup_path(*path_id)),
        CanonNodeKind::MacroCall { path_id, .. } => Some(ir.lookup_path(*path_id)),
        CanonNodeKind::PathRef { path_id } => Some(ir.lookup_path(*path_id)),
        CanonNodeKind::Attr { path_id, .. } => Some(ir.lookup_path(*path_id)),
        _ => None,
    }
}

fn build_span_lookup(bundle: &SymbolSpanBundle) -> HashMap<String, Vec<SpanInfo>> {
    let mut out = HashMap::new();
    for (sym, spans) in &bundle.spans_by_symbol {
        out.entry(sym.clone()).or_insert_with(|| spans.clone());
        if let Some(short) = sym.rsplit("::").next() {
            if !short.is_empty() {
                out.entry(short.to_string())
                    .or_insert_with(|| spans.clone());
            }
        }
    }
    out
}

fn emit_symbol_kinds(writer: &mut TlogWriter, bundle: &SymbolSpanBundle) -> Result<()> {
    for (sym, kind) in &bundle.kinds {
        writer.write_symbol(sym, kind)?;
    }
    Ok(())
}

fn emit_symbol_spans(writer: &mut TlogWriter, bundle: &SymbolSpanBundle) -> Result<()> {
    for (sym, spans) in &bundle.spans_by_symbol {
        for span in spans {
            writer.write_span(sym, span)?;
        }
    }
    Ok(())
}

fn canon_node_kind(kind: &CanonNodeKind) -> NodeKind {
    match kind {
        CanonNodeKind::Crate { .. } => NodeKind::Module,
        CanonNodeKind::Module { .. } => NodeKind::Module,
        CanonNodeKind::Struct { .. } => NodeKind::Struct,
        CanonNodeKind::Enum { .. } => NodeKind::Enum,
        CanonNodeKind::Trait { .. } => NodeKind::Trait,
        CanonNodeKind::AssocType { .. } => NodeKind::Type,
        CanonNodeKind::AssocConst { .. } => NodeKind::Variable,
        CanonNodeKind::Impl { .. } => NodeKind::Impl,
        CanonNodeKind::Fn { .. } => NodeKind::Function,
        CanonNodeKind::FnSig { .. } => NodeKind::Type,
        CanonNodeKind::Type { .. } => NodeKind::Type,
        CanonNodeKind::Field { .. } => NodeKind::Field,
        CanonNodeKind::Param { .. } => NodeKind::Param,
        CanonNodeKind::GenericParam { .. } => NodeKind::Type,
        CanonNodeKind::WherePred { .. } => NodeKind::Type,
        CanonNodeKind::Variant { .. } => NodeKind::Field,
        CanonNodeKind::Attr { .. } => NodeKind::Type,
        CanonNodeKind::Lifetime { .. } => NodeKind::Type,
        CanonNodeKind::Const { .. } => NodeKind::Variable,
        CanonNodeKind::Static { .. } => NodeKind::Variable,
        CanonNodeKind::Use { .. } => NodeKind::Module,
        CanonNodeKind::ExternCrate { .. } => NodeKind::Module,
        CanonNodeKind::TypeAlias { .. } => NodeKind::Type,
        CanonNodeKind::TypeRef { .. } => NodeKind::Type,
        CanonNodeKind::MacroCall { .. } => NodeKind::Module,
        CanonNodeKind::PathRef { .. } => NodeKind::Module,
        CanonNodeKind::Body { .. } => NodeKind::BasicBlock,
        CanonNodeKind::BasicBlock { .. } => NodeKind::BasicBlock,
        CanonNodeKind::MatchArm { .. } => NodeKind::BasicBlock,
        CanonNodeKind::Pattern { .. } => NodeKind::Type,
        CanonNodeKind::VisPath { .. } => NodeKind::Module,
        CanonNodeKind::Local { .. } => NodeKind::Variable,
    }
}

fn emit_ir_edges(
    writer: &mut TlogWriter,
    labels: &[String],
    adj: &canon_ir::csr_graph::CsrGraph<canon_ir::node::CanonId, CanonEdgeKind>,
) -> Result<()> {
    for src_idx in 0..adj.node_data.len() {
        let src_id = adj.node_data[src_idx];
        let start = adj.row_ptr[src_idx] as usize;
        let end = adj.row_ptr[src_idx + 1] as usize;
        for edge_idx in start..end {
            let dst_idx = adj.col_idx[edge_idx] as usize;
            if dst_idx >= adj.node_data.len() {
                return Err(anyhow::anyhow!(
                    "canon invariant violation: edge dst out of range src_idx={src_idx} dst_idx={dst_idx} node_count={}",
                    adj.node_data.len()
                ));
            }
            let dst_id = adj.node_data[dst_idx];
            let edge_kind = match canon_edge_kind(&adj.edge_data[edge_idx]) {
                Some(kind) => kind,
                None => {
                    return Err(anyhow::anyhow!(
                        "canon invariant violation: unhandled edge kind src_idx={src_idx} dst_idx={dst_idx}"
                    ));
                }
            };
            let src = label_for(labels, src_id);
            let dst = label_for(labels, dst_id);
            writer.write_edge(src, dst, edge_kind_str(edge_kind))?;
        }
    }
    Ok(())
}

fn count_ir_call_edges(
    adj: &canon_ir::csr_graph::CsrGraph<canon_ir::node::CanonId, CanonEdgeKind>,
) -> usize {
    adj.edge_data
        .iter()
        .filter(|kind| matches!(kind, CanonEdgeKind::Calls))
        .count()
}

fn emit_block_and_callsite_edges(
    writer: &mut TlogWriter,
    labels: &[String],
    ir: &CanonIR,
) -> Result<()> {
    let mut body_blocks: HashMap<CanonId, Vec<CanonId>> = HashMap::new();
    let mut fn_body: HashMap<CanonId, CanonId> = HashMap::new();
    for node in &ir.nodes {
        if let CanonNodeKind::Fn { body, .. } = &node.kind {
            if let Some(body_id) = body {
                fn_body.insert(node.id, *body_id);
            }
        } else if let CanonNodeKind::Body { blocks } = &node.kind {
            body_blocks.insert(node.id, blocks.clone());
        }
    }

    // Emit HAS_BLOCK edges from function to each basic block in body.
    let mut fn_first_block: HashMap<CanonId, CanonId> = HashMap::new();
    for (fn_id, body_id) in &fn_body {
        if let Some(blocks) = body_blocks.get(body_id) {
            if let Some(first) = blocks.first().copied() {
                fn_first_block.insert(*fn_id, first);
            }
            for block_id in blocks {
                let src = label_for(labels, *fn_id);
                let dst = label_for(labels, *block_id);
                writer.write_edge(src, dst, edge_kind_str(EdgeKind::HasBlock))?;
            }
        }
    }

    // Emit callsite nodes and edges derived from call graph edges.
    let adj = &ir.call_graph;
    let mut callsite_idx_by_fn: HashMap<CanonId, u32> = HashMap::new();
    for src_idx in 0..adj.node_data.len() {
        let src_id = adj.node_data[src_idx];
        let start = adj.row_ptr[src_idx] as usize;
        let end = adj.row_ptr[src_idx + 1] as usize;
        for edge_idx in start..end {
            if !matches!(adj.edge_data[edge_idx], CanonEdgeKind::Calls) {
                continue;
            }
            let dst_idx = adj.col_idx[edge_idx] as usize;
            if dst_idx >= adj.node_data.len() {
                continue;
            }
            let dst_id = adj.node_data[dst_idx];
            let idx = callsite_idx_by_fn.entry(src_id).or_insert(0);
            let callsite_label = format!("callsite::{}::{}", src_id.0, *idx);
            *idx += 1;

            writer.write_node(
                &callsite_label,
                node_kind_str(NodeKind::CallSite),
                "",
                0,
                0,
                0,
                0,
            )?;

            if let Some(block_id) = fn_first_block.get(&src_id) {
                let block_label = label_for(labels, *block_id);
                writer.write_edge(block_label, &callsite_label, edge_kind_str(EdgeKind::HasBlock))?;
            }

            let dst_label = label_for(labels, dst_id);
            writer.write_edge(&callsite_label, dst_label, edge_kind_str(EdgeKind::Call))?;
        }
    }

    Ok(())
}

fn emit_cfg_edges_from_bodies(
    writer: &mut TlogWriter,
    labels: &[String],
    ir: &CanonIR,
) -> Result<()> {
    let mut body_blocks: HashMap<CanonId, Vec<CanonId>> = HashMap::new();
    for node in &ir.nodes {
        if let CanonNodeKind::Body { blocks } = &node.kind {
            body_blocks.insert(node.id, blocks.clone());
        }
    }

    for (body_id, blocks) in &body_blocks {
        let _ = body_id;
        for (idx, block_id) in blocks.iter().enumerate() {
            let node = match ir.nodes.get(block_id.0 as usize) {
                Some(n) => n,
                None => continue,
            };
            let CanonNodeKind::BasicBlock { ops, next } = &node.kind else { continue };

            let mut emit_flow = |target_idx: u32| -> Result<()> {
                let t_idx = target_idx as usize;
                if t_idx >= blocks.len() {
                    return Ok(());
                }
                let src = label_for(labels, *block_id);
                let dst = label_for(labels, blocks[t_idx]);
                writer.write_edge(src, dst, edge_kind_str(EdgeKind::Flow))?;
                Ok(())
            };

            for op in ops {
                match op {
                    canon_ir::node::CfgOp::Branch { true_bb, false_bb, .. } => {
                        emit_flow(*true_bb)?;
                        emit_flow(*false_bb)?;
                    }
                    canon_ir::node::CfgOp::Switch { targets, otherwise, .. } => {
                        for (_, bb) in targets {
                            emit_flow(*bb)?;
                        }
                        if let Some(bb) = otherwise {
                            emit_flow(*bb)?;
                        }
                    }
                    canon_ir::node::CfgOp::Goto(bb) => {
                        emit_flow(*bb)?;
                    }
                    _ => {}
                }
            }

            if let Some(next_idx) = next {
                let _ = emit_flow(*next_idx);
            }

            if idx == blocks.len().saturating_sub(1) {
                // nothing
            }
        }
    }

    Ok(())
}

fn canon_edge_kind(kind: &CanonEdgeKind) -> Option<EdgeKind> {
    match kind {
        CanonEdgeKind::Contains => Some(EdgeKind::Contains),
        CanonEdgeKind::Calls => Some(EdgeKind::Call),
        CanonEdgeKind::CfgEdge | CanonEdgeKind::CfgBranch { .. } => Some(EdgeKind::Flow),
        CanonEdgeKind::TypeOf
        | CanonEdgeKind::TypeUnifies
        | CanonEdgeKind::ImplTrait
        | CanonEdgeKind::DynTrait
        | CanonEdgeKind::Instantiates => Some(EdgeKind::UsesType),
        CanonEdgeKind::ImplRef => Some(EdgeKind::Implements),
        CanonEdgeKind::ImplFor => Some(EdgeKind::ForType),
        CanonEdgeKind::Renames | CanonEdgeKind::Resolves => Some(EdgeKind::Imports),
        CanonEdgeKind::Reexports => Some(EdgeKind::Export),
        CanonEdgeKind::Outlives => Some(EdgeKind::Bounds),
        CanonEdgeKind::ConstDep => Some(EdgeKind::Propagates),
        CanonEdgeKind::Expands => Some(EdgeKind::Contains),
        CanonEdgeKind::AssocItem => Some(EdgeKind::HasMethod),
    }
}

fn node_kind_str(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Function => "FUNCTION",
        NodeKind::Method => "METHOD",
        NodeKind::Struct => "STRUCT",
        NodeKind::Enum => "ENUM",
        NodeKind::Trait => "TRAIT",
        NodeKind::Impl => "IMPL",
        NodeKind::Field => "FIELD",
        NodeKind::Param => "PARAM",
        NodeKind::Variable => "VARIABLE",
        NodeKind::Module => "MODULE",
        NodeKind::Type => "TYPE",
        NodeKind::BasicBlock => "BASIC_BLOCK",
        NodeKind::CallSite => "CALL_SITE",
        NodeKind::Error => "ERROR",
    }
}

fn edge_kind_str(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Contains => "CONTAINS",
        EdgeKind::HasField => "HAS_FIELD",
        EdgeKind::HasMethod => "HAS_METHOD",
        EdgeKind::HasBlock => "HAS_BLOCK",
        EdgeKind::HasParam => "HAS_PARAM",
        EdgeKind::Imports => "IMPORTS",
        EdgeKind::Export => "EXPORT",
        EdgeKind::PublicUse => "PUBLIC_USE",
        EdgeKind::Flow => "FLOW",
        EdgeKind::Call => "CALL",
        EdgeKind::Return => "RETURN",
        EdgeKind::Unwind => "UNWIND",
        EdgeKind::Implements => "IMPLEMENTS",
        EdgeKind::ForType => "FOR_TYPE",
        EdgeKind::UsesType => "USES_TYPE",
        EdgeKind::Bounds => "BOUNDS",
        EdgeKind::Assign => "ASSIGN",
        EdgeKind::Propagates => "PROPAGATES",
        EdgeKind::ArgToParam => "ARG_TO_PARAM",
        EdgeKind::Returns => "RETURNS",
        EdgeKind::ErrorToFunction => "ERROR_TO_FUNCTION",
        EdgeKind::ErrorToBlock => "ERROR_TO_BLOCK",
    }
}

