use crate::types::{BasicBlock, Body, Stmt, Terminator};
use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use std::collections::HashSet;

use crate::capture::mir::analysis as mir_analysis;
use crate::capture::mir::expr as mir_expr;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::patterns as mir_patterns;
use crate::capture::mir::patterns::MirOpKind;
use crate::capture::mir::terminator as mir_terminator;
use crate::capture::mir::util as mir_util;

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

    let switch_analysis = mir_analysis::analyze_switch_structure(body);

    let local_use_counts = mir_util::count_local_uses(body);
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
        if !switch_analysis.switchint_arm_blocks.contains(&idx)
            && !switch_analysis.switch_sources.contains(&idx)
        {
            continue;
        }
        if let Some(term) = &bb.terminator {
            if let mir::TerminatorKind::Call { destination, .. } = &term.kind {
                if let Some(dest_name) = mir_util::label_place_dest(&resolver, destination) {
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
                if let Some(lhs_name) = mir_util::label_place_dest(&resolver, lhs) {
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
        if switch_analysis.switch_sources.contains(&mir_idx_usize) {
            let writes_ret = switch_analysis
                .switch_source_writes_ret
                .get(&mir_idx_usize)
                .copied()
                .unwrap_or(false);
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
        if switch_analysis.switchint_arm_blocks.contains(&mir_idx_usize) {
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
                term = mir_terminator::lower_call_terminator(
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
                term = mir_terminator::lower_non_call_terminator(
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
                        mir_util::emit_suppressed_for_name(
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
                if mir_util::stmt_defines_ret(&field_stmt) {
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
                        mir_util::emit_suppressed_for_name(
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
                if mir_util::stmt_defines_ret(&struct_stmt) {
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
                mir_util::emit_suppressed_for_name(
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
                mir_util::emit_suppressed_for_name(
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
        if mir_util::stmt_defines_ret(&assign_stmt) {
            *ret_value_defined = true;
            *ret_binding_emitted = true;
        }
        if let Stmt::Assign { lhs, .. } = &assign_stmt {
            defined.insert(lhs.clone());
        }
        stmts.push(assign_stmt);
    } else if let Some(lhs_name) = lhs_name {
        mir_util::emit_suppressed_for_name(
            &lhs_name,
            stmts,
            defined,
            suppressed_sentinel_names,
            ret_value_defined,
            ret_binding_emitted,
        );
    }
}
