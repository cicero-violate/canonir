use model::ir::edge::{EdgeHint, EdgeKind};
use rustc_hir as hir;
use rustc_middle::mir::{self};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::index::Index;

/// MIR/body projection: emit CFG edges, call edges, and const deps as EdgeHints.
///
/// All edges emitted here use the function's NodeId (`id`) as `src`.
/// MIR basic-block indices are NOT NodeIds and must never appear in EdgeHint fields.
///
/// CFG edges: self-loop on `id` until BB-level nodes exist in ModelIR.
/// Call edges: src=caller id, dst=callee NodeId (skipped if callee not in index).
/// ConstDep:   src=caller id, dst=const NodeId (skipped if const not in index).
pub fn project_body(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Vec<EdgeHint> {
    let Some(local_def) = def_id.as_local() else {
        return Vec::new();
    };
    let Some(&id) = index.def_to_node.get(&def_id) else {
        return Vec::new();
    };

    // Guard: only project bodies for items that actually have MIR.
    if !tcx.is_mir_available(local_def) {
        return Vec::new();
    }
    // optimized_mir panics on Const/Static items — use mir_for_ctfe for those.
    let body = match tcx.hir_body_const_context(local_def) {
        Some(hir::ConstContext::ConstFn) | Some(hir::ConstContext::Const { .. }) | Some(hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };

    let caller_id = id.index() as u32;
    let mut hints = Vec::new();

    for bb_data in body.basic_blocks.iter() {
        // CFG edges: record that control flow exists within this function.
        // src=dst=caller_id (self-loop) until ModelIR grows BB-level nodes.
        if let Some(term) = &bb_data.terminator {
            for _succ in term.successors() {
                hints.push(EdgeHint { src: caller_id, dst: caller_id, kind: EdgeKind::CfgEdge });
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
                        if let Some(&const_node) = index.def_to_node.get(&uneval.def) {
                            if const_node != id {
                                hints.push(EdgeHint { src: caller_id, dst: const_node.index() as u32, kind: EdgeKind::ConstDep });
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
                    if let Some(&callee_node) = index.def_to_node.get(&callee_def_id) {
                        hints.push(EdgeHint { src: caller_id, dst: callee_node.index() as u32, kind: EdgeKind::Calls });
                    }
                }
            }
        }
    }

    hints
}
