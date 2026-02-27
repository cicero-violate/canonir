use crate::types::{BasicBlock, Body, Stmt, Terminator};
use rustc_middle::mir;
use rustc_middle::mir::visit::Visitor;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::capture::mir::expr as mir_expr;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::patterns as mir_patterns;
use crate::capture::mir::patterns::MirOpKind;

use super::resolver::LocalNameResolver;

pub(crate) fn mir_body_structural(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    param_names: &[String],
    returns_unit: bool,
) -> Body {
    let Some(local_def) = def_id.as_local() else {
        return Body::None;
    };
    if !tcx.is_mir_available(local_def) {
        return Body::None;
    }
    let body = match tcx.hir_body_const_context(local_def) {
        Some(rustc_hir::ConstContext::ConstFn)
        | Some(rustc_hir::ConstContext::Const { .. })
        | Some(rustc_hir::ConstContext::Static(_)) => tcx.mir_for_ctfe(local_def),
        None => tcx.optimized_mir(local_def),
    };
    let resolver = LocalNameResolver::new(body, param_names);
    let mut defined: HashSet<String> = param_names.iter().cloned().collect();
    defined.insert("__ret".to_string());
    let mut ret_value_defined = false;
    let mut ret_binding_emitted = false;
    let mut match_dest_emitted = false;

    let mut mir_to_emitted: Vec<Option<u32>> = vec![None; body.basic_blocks.len()];
    let mut next_emitted = 0u32;
    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        mir_to_emitted[mir_idx.as_usize()] = Some(next_emitted);
        next_emitted += 1;
    }

    let mut switch_sources: BTreeSet<usize> = BTreeSet::new();
    let mut direct_switch_succ: BTreeSet<usize> = BTreeSet::new();
    let mut preds: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); body.basic_blocks.len()];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); body.basic_blocks.len()];
    for (idx, bb) in body.basic_blocks.iter_enumerated() {
        let Some(term) = &bb.terminator else {
            continue;
        };
        for succ in term.successors() {
            succs[idx.as_usize()].push(succ.as_usize());
            preds[succ.as_usize()].insert(idx.as_usize());
        }
        if matches!(term.kind, mir::TerminatorKind::SwitchInt { .. }) {
            switch_sources.insert(idx.as_usize());
            for succ in term.successors() {
                direct_switch_succ.insert(succ.as_usize());
            }
        }
    }
    let mut switch_reachable: BTreeSet<usize> = direct_switch_succ.clone();
    let mut frontier: Vec<usize> = direct_switch_succ.iter().copied().collect();
    while let Some(cur) = frontier.pop() {
        for sidx in &succs[cur] {
            let sidx = *sidx;
            if switch_reachable.insert(sidx) {
                frontier.push(sidx);
            }
        }
    }

    let mut switchint_arm_blocks: BTreeSet<usize> = BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for bb_idx in 0..body.basic_blocks.len() {
            if switchint_arm_blocks.contains(&bb_idx) || !switch_reachable.contains(&bb_idx) {
                continue;
            }
            let incoming = &preds[bb_idx];
            if incoming.is_empty() {
                continue;
            }
            let exclusively_switch_reachable = incoming.iter().all(|p| {
                switch_sources.contains(p)
                    || switchint_arm_blocks.contains(p)
                    || direct_switch_succ.contains(p)
            });
            if exclusively_switch_reachable {
                switchint_arm_blocks.insert(bb_idx);
                changed = true;
            }
        }
    }

    let bb_writes_ret: Vec<bool> = body.basic_blocks.iter().map(bb_writes_return_place).collect();
    let mut switch_source_writes_ret: HashMap<usize, bool> = HashMap::new();
    for src in &switch_sources {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut stack: Vec<usize> = succs[*src].clone();
        let mut writes_ret = false;
        while let Some(cur) = stack.pop() {
            if !switch_reachable.contains(&cur) || !seen.insert(cur) {
                continue;
            }
            if bb_writes_ret.get(cur).copied().unwrap_or(false) {
                writes_ret = true;
                break;
            }
            for next in &succs[cur] {
                if switch_reachable.contains(next) {
                    stack.push(*next);
                }
            }
        }
        switch_source_writes_ret.insert(*src, writes_ret);
    }

    let local_use_counts = count_local_uses(body);
    let mut filtered_arg_locals: HashSet<u32> = HashSet::new();
    for bb in body.basic_blocks.iter() {
        let Some(term_ref) = &bb.terminator else {
            continue;
        };
        let mir::TerminatorKind::Call { func, args, .. } = &term_ref.kind else {
            continue;
        };
        if !mir_ops::filtered_internal_call_target(tcx, func) {
            continue;
        }
        for arg in args {
            if let mir::Operand::Copy(place) | mir::Operand::Move(place) = &arg.node {
                filtered_arg_locals.insert(place.local.as_u32());
            }
        }
    }
    let mut call_feed_locals: HashSet<String> = HashSet::new();
    for local_u32 in filtered_arg_locals {
        if local_use_counts.get(&local_u32).copied().unwrap_or(0) != 1 {
            continue;
        }
        let local = mir::Local::from_u32(local_u32);
        if let Some(name) = resolver.label_local(local) {
            call_feed_locals.insert(name);
        }
    }

    let mut suppressed_dest_sentinels: Vec<Stmt> = Vec::new();
    let mut suppressed_sentinel_names: HashSet<String> = HashSet::new();
    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        let idx = bb_idx.as_usize();
        if !switchint_arm_blocks.contains(&idx) && !switch_sources.contains(&idx) {
            continue;
        }
        if let Some(term) = &bb.terminator {
            if let mir::TerminatorKind::Call { destination, .. } = &term.kind {
                if let Some(dest_name) = label_place_dest(&resolver, destination) {
                    mir_guard::emit_suppressed_binding(
                        &dest_name,
                        &mut defined,
                        &mut suppressed_sentinel_names,
                        &mut suppressed_dest_sentinels,
                    );
                }
            }
        }
        for stmt in &bb.statements {
            if let mir::StatementKind::Assign(boxed) = &stmt.kind {
                let (lhs, _) = &**boxed;
                if let Some(lhs_name) = label_place_dest(&resolver, lhs) {
                    mir_guard::emit_suppressed_binding(
                        &lhs_name,
                        &mut defined,
                        &mut suppressed_sentinel_names,
                        &mut suppressed_dest_sentinels,
                    );
                }
            }
        }
    }

    let mut sentinels_injected = false;
    let mut blocks: Vec<BasicBlock> = Vec::with_capacity(next_emitted as usize);
    for (mir_idx, bb) in body.basic_blocks.iter_enumerated() {
        if bb.is_cleanup {
            continue;
        }
        let mir_idx_usize = mir_idx.as_usize();
        if switch_sources.contains(&mir_idx_usize) {
            let writes_ret = switch_source_writes_ret.get(&mir_idx_usize).copied().unwrap_or(false);
            let dest = if !returns_unit && writes_ret && !match_dest_emitted {
                ret_value_defined = true;
                ret_binding_emitted = true;
                defined.insert("__ret".to_string());
                match_dest_emitted = true;
                Some("__ret".to_string())
            } else {
                None
            };
            blocks.push(BasicBlock {
                stmts: vec![Stmt::Match { dest }],
                terminator: Terminator::Unreachable,
            });
            continue;
        }
        if switchint_arm_blocks.contains(&mir_idx_usize) {
            blocks.push(BasicBlock {
                stmts: Vec::new(),
                terminator: Terminator::Unreachable,
            });
            continue;
        }

        let mut stmts: Vec<Stmt> = Vec::new();
        if !sentinels_injected && !suppressed_dest_sentinels.is_empty() {
            stmts.extend(suppressed_dest_sentinels.drain(..));
            sentinels_injected = true;
        }

        for stmt in &bb.statements {
            lower_assign_statement(
                tcx,
                &body.local_decls,
                stmt,
                &resolver,
                &call_feed_locals,
                &mut defined,
                &mut suppressed_sentinel_names,
                &mut stmts,
                &mut ret_value_defined,
                &mut ret_binding_emitted,
                &mut match_dest_emitted,
            );
        }

        let mut term = Terminator::None;
        if let Some(term_ref) = &bb.terminator {
            if let mir::TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } = &term_ref.kind
            {
                term = lower_call_terminator(
                    tcx,
                    func,
                    args,
                    destination,
                    *target,
                    &resolver,
                    &mir_to_emitted,
                    &mut stmts,
                    &mut defined,
                    &mut suppressed_sentinel_names,
                    &mut ret_value_defined,
                    &mut ret_binding_emitted,
                    &mut match_dest_emitted,
                );
            } else {
                term = lower_non_call_terminator(
                    tcx,
                    term_ref,
                    returns_unit,
                    &resolver,
                    &mir_to_emitted,
                    &mut stmts,
                    &mut defined,
                    &mut ret_value_defined,
                    &mut ret_binding_emitted,
                    &mut match_dest_emitted,
                );
            }
        }

        blocks.push(BasicBlock { stmts, terminator: term });
    }

    Body::Blocks(blocks)
}

fn emit_suppressed_for_name(
    name: &str,
    stmts: &mut Vec<Stmt>,
    defined: &mut HashSet<String>,
    suppressed_sentinel_names: &mut HashSet<String>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
) {
    if name == "__ret" {
        emit_suppressed_ret_binding(stmts, defined, ret_value_defined, ret_binding_emitted);
    } else {
        mir_guard::emit_suppressed_binding(name, defined, suppressed_sentinel_names, stmts);
    }
}

fn lower_assign_statement<'tcx>(
    tcx: TyCtxt<'tcx>,
    local_decls: &mir::LocalDecls<'tcx>,
    stmt: &mir::Statement<'tcx>,
    resolver: &LocalNameResolver,
    call_feed_locals: &HashSet<String>,
    defined: &mut HashSet<String>,
    suppressed_sentinel_names: &mut HashSet<String>,
    stmts: &mut Vec<Stmt>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
    match_dest_emitted: &mut bool,
) {
    let mir::StatementKind::Assign(boxed) = &stmt.kind else {
        return;
    };
    let (lhs, rvalue) = &**boxed;
    let lhs_name = resolver.label_place(lhs);
    if lhs_name.as_ref().is_some_and(|name| call_feed_locals.contains(name)) {
        return;
    }

    match mir_patterns::dispatch_stmt_pattern(tcx, rvalue) {
        MirOpKind::FieldAccess => {
            if let Some(field_stmt) =
                mir_expr::mir_field_access_stmt(tcx, local_decls, lhs, rvalue, resolver)
            {
                if !mir_guard::structural_guard(&field_stmt, defined, suppressed_sentinel_names) {
                    if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                        emit_suppressed_for_name(
                            dest,
                            stmts,
                            defined,
                            suppressed_sentinel_names,
                            ret_value_defined,
                            ret_binding_emitted,
                        );
                    }
                    return;
                }
                if stmt_defines_ret(&field_stmt) {
                    *ret_value_defined = true;
                    *ret_binding_emitted = true;
                }
                if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                    defined.insert(dest.clone());
                }
                stmts.push(field_stmt);
                return;
            }
            if let Some(lhs_name) = resolver.label_place(lhs) {
                defined.insert(lhs_name.clone());
                if lhs_name == "__ret" {
                    *ret_value_defined = true;
                }
            }
            return;
        }
        MirOpKind::StructLit => {
            if let Some(struct_stmt) = mir_expr::mir_struct_lit_stmt(tcx, lhs, rvalue, resolver) {
                if !mir_guard::structural_guard(&struct_stmt, defined, suppressed_sentinel_names) {
                    if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                        emit_suppressed_for_name(
                            dest,
                            stmts,
                            defined,
                            suppressed_sentinel_names,
                            ret_value_defined,
                            ret_binding_emitted,
                        );
                    }
                    return;
                }
                if stmt_defines_ret(&struct_stmt) {
                    *ret_value_defined = true;
                    *ret_binding_emitted = true;
                }
                if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                    defined.insert(dest.clone());
                }
                stmts.push(struct_stmt);
            }
            return;
        }
        MirOpKind::OpaqueAggregate => {
            if let Some(lhs_name) = lhs_name.clone() {
                defined.insert(lhs_name.clone());
                if lhs_name == "__ret" {
                    *ret_value_defined = true;
                    if !*match_dest_emitted {
                        stmts.push(Stmt::Match {
                            dest: Some("__ret".to_string()),
                        });
                        *match_dest_emitted = true;
                        *ret_binding_emitted = true;
                    }
                }
            }
            return;
        }
        MirOpKind::ZeroArgEnumCtor => {
            if let Some(lhs_name) = lhs_name.clone() {
                emit_suppressed_for_name(
                    &lhs_name,
                    stmts,
                    defined,
                    suppressed_sentinel_names,
                    ret_value_defined,
                    ret_binding_emitted,
                );
            }
            return;
        }
        MirOpKind::ConstUse => {
            // Fall through to generic assign lowering for non-zero-arg const uses.
        }
        MirOpKind::ArrayAggregate => {
            if let Some(lhs_name) = lhs_name.as_ref() && defined.contains(lhs_name) {
                return;
            }
            // Fall through when destination is not yet defined.
        }
        MirOpKind::Assign => {}
    }

    if let Some(assign_stmt) = mir_expr::mir_assign_stmt(
        tcx,
        local_decls,
        lhs,
        rvalue,
        resolver,
        defined,
        suppressed_sentinel_names,
    ) {
        if !mir_guard::structural_guard(&assign_stmt, defined, suppressed_sentinel_names) {
            if let Stmt::Assign { lhs, .. } = &assign_stmt {
                emit_suppressed_for_name(
                    lhs,
                    stmts,
                    defined,
                    suppressed_sentinel_names,
                    ret_value_defined,
                    ret_binding_emitted,
                );
            }
            return;
        }
        if stmt_defines_ret(&assign_stmt) {
            *ret_value_defined = true;
            *ret_binding_emitted = true;
        }
        if let Stmt::Assign { lhs, .. } = &assign_stmt {
            defined.insert(lhs.clone());
        }
        stmts.push(assign_stmt);
    } else if let Some(lhs_name) = lhs_name {
        emit_suppressed_for_name(
            &lhs_name,
            stmts,
            defined,
            suppressed_sentinel_names,
            ret_value_defined,
            ret_binding_emitted,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    destination: &mir::Place<'tcx>,
    target: Option<mir::BasicBlock>,
    resolver: &LocalNameResolver,
    mir_to_emitted: &[Option<u32>],
    stmts: &mut Vec<Stmt>,
    defined: &mut HashSet<String>,
    suppressed_sentinel_names: &mut HashSet<String>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
    match_dest_emitted: &mut bool,
) -> Terminator {
    if mir_ops::filtered_internal_call_target(tcx, func) {
        if let Some(dest) = label_place_dest(resolver, destination) {
            emit_suppressed_for_name(
                &dest,
                stmts,
                defined,
                suppressed_sentinel_names,
                ret_value_defined,
                ret_binding_emitted,
            );
        }
    } else if let Some(dest) = label_place_dest(resolver, destination)
        && let Some(method_stmt) = mir_ops::mir_method_call_stmt(tcx, func, args, resolver, dest.clone())
    {
        if !mir_guard::structural_guard(&method_stmt, defined, suppressed_sentinel_names) {
            if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
                emit_suppressed_for_name(
                    dest,
                    stmts,
                    defined,
                    suppressed_sentinel_names,
                    ret_value_defined,
                    ret_binding_emitted,
                );
            }
            return target
                .and_then(|bb| remap_bb_target(bb, mir_to_emitted))
                .map(Terminator::Goto)
                .unwrap_or(Terminator::None);
        }
        if stmt_defines_ret(&method_stmt) {
            *ret_value_defined = true;
            *ret_binding_emitted = true;
        }
        if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
            defined.insert(dest.clone());
        }
        stmts.push(method_stmt);
    } else if let Some(dest) = label_place_dest(resolver, destination)
        && let Some(call_stmt) = mir_ops::mir_call_stmt(tcx, func, args, resolver, dest.clone())
    {
        if mir_guard::structural_guard(&call_stmt, defined, suppressed_sentinel_names) {
            if stmt_defines_ret(&call_stmt) {
                *ret_value_defined = true;
                *ret_binding_emitted = true;
            }
            if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
                defined.insert(dest.clone());
            }
            stmts.push(call_stmt);
        } else if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
            emit_suppressed_for_name(
                dest,
                stmts,
                defined,
                suppressed_sentinel_names,
                ret_value_defined,
                ret_binding_emitted,
            );
        }
    } else if let Some(dest_name) = label_place_dest(resolver, destination) {
        if dest_name != "__ret" {
            mir_guard::emit_suppressed_binding(
                &dest_name,
                defined,
                suppressed_sentinel_names,
                stmts,
            );
        } else {
            if !*match_dest_emitted {
                stmts.push(Stmt::Match {
                    dest: Some("__ret".to_string()),
                });
                *match_dest_emitted = true;
                *ret_value_defined = true;
                *ret_binding_emitted = true;
            }
            defined.insert("__ret".to_string());
        }
    }

    target
        .and_then(|bb| remap_bb_target(bb, mir_to_emitted))
        .map(Terminator::Goto)
        .unwrap_or(Terminator::None)
}

#[allow(clippy::too_many_arguments)]
fn lower_non_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>,
    term_ref: &mir::Terminator<'tcx>,
    returns_unit: bool,
    resolver: &LocalNameResolver,
    mir_to_emitted: &[Option<u32>],
    stmts: &mut Vec<Stmt>,
    defined: &mut HashSet<String>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
    match_dest_emitted: &mut bool,
) -> Terminator {
    match &term_ref.kind {
        mir::TerminatorKind::Return => {
            lower_return_terminator(
                returns_unit,
                stmts,
                defined,
                ret_value_defined,
                ret_binding_emitted,
                match_dest_emitted,
            );
            Terminator::None
        }
        mir::TerminatorKind::Goto { target }
        | mir::TerminatorKind::Drop { target, .. }
        | mir::TerminatorKind::Assert { target, .. } => remap_to_goto(*target, mir_to_emitted),
        mir::TerminatorKind::SwitchInt { discr, .. } => {
            let mut succ = term_ref.successors();
            if let (Some(t), Some(f)) = (succ.next(), succ.next()) {
                if let Some(cond) = mir_ops::mir_operand_label(tcx, discr, resolver) {
                    let true_bb = remap_bb_target(t, mir_to_emitted);
                    let false_bb = remap_bb_target(f, mir_to_emitted);
                    return match (true_bb, false_bb) {
                        (Some(t), Some(f)) => Terminator::Branch {
                            cond,
                            true_bb: t,
                            false_bb: f,
                        },
                        (Some(t), None) => Terminator::Goto(t),
                        (None, Some(f)) => Terminator::Goto(f),
                        (None, None) => Terminator::None,
                    };
                }
            }
            Terminator::None
        }
        _ => Terminator::None,
    }
}

fn remap_to_goto(target: mir::BasicBlock, mir_to_emitted: &[Option<u32>]) -> Terminator {
    remap_bb_target(target, mir_to_emitted)
        .map(Terminator::Goto)
        .unwrap_or(Terminator::None)
}

fn lower_return_terminator(
    returns_unit: bool,
    stmts: &mut Vec<Stmt>,
    defined: &mut HashSet<String>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
    match_dest_emitted: &mut bool,
) {
    if returns_unit {
        stmts.push(Stmt::Return(None));
    } else if *ret_binding_emitted && !*match_dest_emitted {
        stmts.push(Stmt::Return(Some("__ret".to_string())));
    } else if !*match_dest_emitted && (!*ret_value_defined || !*ret_binding_emitted) {
        stmts.push(Stmt::Match {
            dest: Some("__ret".to_string()),
        });
        *match_dest_emitted = true;
        *ret_value_defined = true;
        *ret_binding_emitted = true;
        defined.insert("__ret".to_string());
    }
}

pub(crate) fn label_place_dest(
    resolver: &LocalNameResolver,
    place: &mir::Place<'_>,
) -> Option<String> {
    if let Some(name) = resolver.label_place(place) {
        return Some(name);
    }
    let has_unsafe_proj = place.projection.iter().any(|p| {
        matches!(
            p,
            mir::ProjectionElem::Downcast(..)
                | mir::ProjectionElem::OpaqueCast(..)
                | mir::ProjectionElem::UnwrapUnsafeBinder(..)
        )
    });
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

pub(crate) fn emit_suppressed_ret_binding(
    stmts: &mut Vec<Stmt>,
    defined: &mut HashSet<String>,
    ret_value_defined: &mut bool,
    ret_binding_emitted: &mut bool,
) {
    stmts.push(Stmt::Assign {
        lhs: "__ret".to_string(),
        rhs: "__canon_suppressed__".to_string(),
    });
    *ret_value_defined = true;
    *ret_binding_emitted = true;
    defined.insert("__ret".to_string());
}

pub(crate) fn is_method_call_candidate(tcx: TyCtxt<'_>, func: &mir::Operand<'_>) -> bool {
    func.const_fn_def().map(|(did, _)| matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn)).unwrap_or(false)
}

pub(crate) fn count_local_uses<'tcx>(body: &mir::Body<'tcx>) -> HashMap<u32, usize> {
    struct Counter {
        counts: HashMap<u32, usize>,
    }
    impl<'tcx> Visitor<'tcx> for Counter {
        fn visit_local(
            &mut self,
            local: mir::Local,
            context: rustc_middle::mir::visit::PlaceContext,
            location: rustc_middle::mir::Location,
        ) {
            if context.is_use() {
                *self.counts.entry(local.as_u32()).or_insert(0) += 1;
            }
            self.super_local(local, context, location);
        }
    }
    let mut counter = Counter {
        counts: HashMap::new(),
    };
    counter.visit_body(body);
    counter.counts
}
