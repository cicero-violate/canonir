use crate::types::{EdgeHint, Node, NodeId, NodeKind};
use rustc_hir as hir;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::mir::{self};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::def_id::LOCAL_CRATE;

use crate::capture::edge_emit;
use crate::index::Index;
use crate::norm;

/// MIR/body projection: emit CFG edges, call edges, and const deps as EdgeHints.
///
/// All edges emitted here use the function's NodeId (`id`) as `src`.
/// MIR basic-block indices are NOT NodeIds and must never appear in EdgeHint fields.
///
/// CFG edges: self-loop on `id` until BB-level nodes exist in ModelIR.
/// Call edges: src=caller id, dst=callee NodeId (skipped if callee not in index).
/// ConstDep:   src=caller id, dst=const NodeId (skipped if const not in index).
/// PathRef:    structural external paths discovered from MIR def references.
pub fn project_body(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> (Vec<Node>, Vec<EdgeHint>) {
    let Some(local_def) = def_id.as_local() else {
        return (Vec::new(), Vec::new());
    };
    let Some(&id) = index.def_to_node.get(&def_id) else {
        return (Vec::new(), Vec::new());
    };

    // Guard: only project bodies for items that actually have MIR.
    if !tcx.is_mir_available(local_def) {
        return (Vec::new(), Vec::new());
    }
    // optimized_mir panics on Const/Static items — use mir_for_ctfe for those.
    let body = match tcx.hir_body_const_context(local_def) {
        Some(hir::ConstContext::ConstFn) | Some(hir::ConstContext::Const { .. }) | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };

    let caller_id = id.index() as u32;
    let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
    let mut pathrefs: Vec<String> = Vec::new();
    let mut hints = Vec::new();

    // Collect external DefId paths referenced anywhere in MIR operands.
    let mut collector = ExternalDefCollector { tcx, crate_name: &crate_name, out: &mut pathrefs };
    collector.visit_body(body);

    for bb_data in body.basic_blocks.iter() {
        // CFG edges: record that control flow exists within this function.
        // src=dst=caller_id (self-loop) until ModelIR grows BB-level nodes.
        if let Some(term) = &bb_data.terminator {
            for _succ in term.successors() {
                edge_emit::push(&mut hints, caller_id, caller_id, crate::types::EdgeKind::CfgEdge);
            }
        }

        // ConstDep: caller depends on a referenced const item.
        // Only emit when the const operand resolves to a known NodeId.
        for stmt in &bb_data.statements {
            if let mir::StatementKind::Assign(boxed) = &stmt.kind {
                let (_, rvalue) = &**boxed;
                if let mir::Rvalue::Use(mir::Operand::Constant(c)) = rvalue {
                    // Only unevaluated consts carry a DefId we can resolve.
                    if let mir::Const::Unevaluated(uneval, _) = c.const_ {
                        push_external_path(tcx, uneval.def, &crate_name, &mut pathrefs);
                        if let Some(&const_node) = index.def_to_node.get(&uneval.def) {
                            if const_node != id {
                                edge_emit::push(&mut hints, caller_id, const_node.index() as u32, crate::types::EdgeKind::ConstDep);
                            }
                        }
                    }
                }
            }
        }

        // Call edges: src=caller NodeId, dst=callee NodeId.
        if let Some(term) = &bb_data.terminator {
            if let mir::TerminatorKind::Call { func, .. } = &term.kind {
                if let Some((callee_def_id, _)) = func.const_fn_def() {
                    push_external_path(tcx, callee_def_id, &crate_name, &mut pathrefs);
                    if let Some(&callee_node) = index.def_to_node.get(&callee_def_id) {
                        edge_emit::push(&mut hints, caller_id, callee_node.index() as u32, crate::types::EdgeKind::Calls);
                    }
                }
            }
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    for (ordinal, path) in pathrefs.into_iter().enumerate() {
        let node_id = synthetic_body_pathref_id(id, ordinal as u32);
        nodes.push(Node { id: node_id, kind: NodeKind::PathRef { path }, span: None });
        edge_emit::push_contains(&mut hints, caller_id, node_id.index() as u32);
    }

    (nodes, hints)
}

fn synthetic_body_pathref_id(base: NodeId, ordinal: u32) -> NodeId {
    NodeId(1_100_000_000u32 + base.0.saturating_mul(1024) + ordinal)
}

fn push_external_path(tcx: TyCtxt<'_>, did: DefId, crate_name: &str, out: &mut Vec<String>) {
    let path = norm::path(tcx, did);
    if path.is_empty()
        || path.starts_with("crate::")
        || path.starts_with("self::")
        || path.starts_with("super::")
    {
        return;
    }
    let root = path.split("::").next().unwrap_or("").trim();
    if root.is_empty() || root == crate_name {
        return;
    }
    // Prevent private/helper segments (e.g., `_serde`, `_foo`) from
    // crossing the capture boundary and reaching Canon path interner.
    if path.split("::").any(|seg| seg.starts_with('_')) {
        return;
    }
    if matches!(root, "std" | "core" | "alloc" | "proc_macro" | "crate" | "self" | "super") {
        return;
    }
    if !out.iter().any(|p| p == &path) {
        out.push(path);
    }
}

struct ExternalDefCollector<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    crate_name: &'a str,
    out: &'a mut Vec<String>,
}

impl<'tcx> Visitor<'tcx> for ExternalDefCollector<'_, 'tcx> {
    fn visit_operand(&mut self, operand: &mir::Operand<'tcx>, location: mir::Location) {
        if let mir::Operand::Constant(c) = operand
            && let mir::Const::Unevaluated(uneval, _) = c.const_
        {
            push_external_path(self.tcx, uneval.def, self.crate_name, self.out);
        }
        self.super_operand(operand, location);
    }
}
