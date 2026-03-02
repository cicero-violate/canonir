use rustc_middle::mir;
use rustc_middle::mir::visit::Visitor;
use std::collections::{HashMap, HashSet};

use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::types::Stmt;

pub(crate) fn label_place_dest(resolver: &LocalNameResolver, place: &mir::Place<'_>) -> Option<String> {
    if let Some(name) = resolver.label_place(place) {
        return Some(name);
    }
    let has_unsafe_proj = place.projection.iter().any(|p| matches!(p, mir::ProjectionElem::Downcast(..) | mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..)));
    if has_unsafe_proj {
        return None;
    }
    resolver.label_local(place.local)
}

pub(crate) fn remap_bb_target(target: mir::BasicBlock, mir_to_emitted: &[Option<u32>]) -> Option<u32> {
    mir_to_emitted.get(target.as_usize()).and_then(|slot| *slot)
}

pub(crate) fn bb_writes_return_place(bb: &mir::BasicBlockData<'_>) -> bool {
    for stmt in &bb.statements {
        let mir::StatementKind::Assign(boxed) = &stmt.kind else {
            continue;
        };
        let (lhs, _) = &**boxed;
        if lhs.local.as_u32() == 0 {
            return true;
        }
    }
    let Some(term) = &bb.terminator else {
        return false;
    };
    if let mir::TerminatorKind::Call { destination, .. } = &term.kind {
        return destination.local.as_u32() == 0;
    }
    false
}

pub(crate) fn stmt_defines_ret(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Assign { lhs, .. } => lhs == "__ret",
        Stmt::Call { dest: Some(dest), .. } => dest == "__ret",
        Stmt::FieldAccess { dest: Some(dest), .. } => dest == "__ret",
        Stmt::MethodCall { dest: Some(dest), .. } => dest == "__ret",
        Stmt::StructLit { dest: Some(dest), .. } => dest == "__ret",
        Stmt::Match { dest: Some(dest) } => dest == "__ret",
        _ => false,
    }
}

pub(crate) fn emit_suppressed_ret_binding(stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>) {
    // Structural invariant: never synthesize suppressed return bindings.
    // Deterministic return fallback in lowering must handle missing __ret.
}

pub(crate) fn emit_suppressed_for_name(name: &str, stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>, suppressed_sentinel_names: &mut HashSet<String>) {
    // Never suppress __ret; return must be lowered structurally.
    if name == "__ret" {
        return;
    }
    // Suppressed bindings are eliminated at this stage.
    // Structural lowering and deterministic return fallback
    // are responsible for maintaining invariants.
}

pub(crate) fn count_local_uses<'tcx>(body: &mir::Body<'tcx>) -> HashMap<u32, usize> {
    struct Counter {
        counts: HashMap<u32, usize>,
    }
    impl<'tcx> Visitor<'tcx> for Counter {
        fn visit_local(&mut self, local: mir::Local, context: rustc_middle::mir::visit::PlaceContext, location: rustc_middle::mir::Location) {
            if context.is_use() {
                *self.counts.entry(local.as_u32()).or_insert(0) += 1;
            }
            self.super_local(local, context, location);
        }
    }
    let mut counter = Counter { counts: HashMap::new() };
    counter.visit_body(body);
    counter.counts
}
