use crate::types::TraitMethod;
use crate::types::{BasicBlock, Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, PrimType, Stmt, StructKind, Terminator, TypeExpr, Visibility};
use crate::types::{EdgeHint, EdgeKind};
use rustc_hir::{def::DefKind, GenericBound, PatKind, PredicateOrigin, Safety, WherePredicateKind};
use rustc_middle::mir::{self};
use rustc_middle::mir::visit::Visitor;
use rustc_middle::ty::print::PrintTraitRefExt;
use rustc_middle::ty::AssocKind;
use rustc_middle::ty::{self, CoroutineArgsExt, TyCtxt};
use rustc_span::def_id::DefId;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::index::Index;
use crate::norm;
use crate::project::engine;

/// Structural projection: DefId -> NodeKind using HIR/ty queries.
/// All strings are canonicalized via norm:: before NodeKind construction.
pub fn project_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> (Vec<Node>, Vec<EdgeHint>) {
    if let Some(out) = engine::lower_def(tcx, def_id, index) {
        return out;
    }
    project_item_legacy(tcx, def_id, index)
}

pub(crate) fn project_item_legacy(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> (Vec<Node>, Vec<EdgeHint>) {
    let _ = tcx;
    let _ = def_id;
    let _ = index;
    (Vec::new(), Vec::new())
}

use crate::project::helpers::{lower_ty, render_type_expr};

pub(crate) fn mir_body_structural(tcx: TyCtxt<'_>, def_id: DefId, param_names: &[String], returns_unit: bool) -> Body {
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
        if !filtered_internal_call_target(tcx, func, args, &resolver) {
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
                    if dest_name != "__ret"
                        && !defined.contains(&dest_name)
                        && suppressed_sentinel_names.insert(dest_name.clone())
                    {
                        defined.insert(dest_name.clone());
                        suppressed_dest_sentinels.push(Stmt::Assign {
                            lhs: dest_name,
                            rhs: "__canon_suppressed__".to_string(),
                        });
                    }
                }
            }
        }
        for stmt in &bb.statements {
            if let mir::StatementKind::Assign(boxed) = &stmt.kind {
                let (lhs, _) = &**boxed;
                if let Some(lhs_name) = label_place_dest(&resolver, lhs) {
                    if lhs_name != "__ret"
                        && !defined.contains(&lhs_name)
                        && suppressed_sentinel_names.insert(lhs_name.clone())
                    {
                        defined.insert(lhs_name.clone());
                        suppressed_dest_sentinels.push(Stmt::Assign {
                            lhs: lhs_name,
                            rhs: "__canon_suppressed__".to_string(),
                        });
                    }
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
            let mir::StatementKind::Assign(boxed) = &stmt.kind else {
                continue;
            };
            let (lhs, rvalue) = &**boxed;
            let lhs_name = resolver.label_place(lhs);
            if lhs_name.as_ref().is_some_and(|name| call_feed_locals.contains(name)) {
                continue;
            }
            if is_field_access_candidate(rvalue) {
                if let Some(field_stmt) = mir_field_access_stmt(tcx, &body.local_decls, lhs, rvalue, &resolver) {
                    if !stmt_inputs_known(&field_stmt, &defined, &suppressed_sentinel_names) {
                        if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                            if dest != "__ret" && !defined.contains(dest) {
                                defined.insert(dest.clone());
                                suppressed_sentinel_names.insert(dest.clone());
                                stmts.push(Stmt::Assign {
                                    lhs: dest.clone(),
                                    rhs: "__canon_suppressed__".to_string(),
                                });
                            }
                        }
                        continue;
                    }
                    if stmt_defines_ret(&field_stmt) {
                        ret_value_defined = true;
                        ret_binding_emitted = true;
                    }
                    if let Stmt::FieldAccess { dest: Some(dest), .. } = &field_stmt {
                        defined.insert(dest.clone());
                    }
                    stmts.push(field_stmt);
                    continue;
                }
                if let Some(lhs_name) = resolver.label_place(lhs) {
                    defined.insert(lhs_name.clone());
                    if lhs_name == "__ret" {
                        ret_value_defined = true;
                    }
                }
                continue;
            }
            if is_struct_lit_candidate(rvalue) {
                if let Some(struct_stmt) = mir_struct_lit_stmt(tcx, lhs, rvalue, &resolver) {
                    if !stmt_inputs_known(&struct_stmt, &defined, &suppressed_sentinel_names) {
                        if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                            if dest != "__ret" && !defined.contains(dest) {
                                defined.insert(dest.clone());
                                suppressed_sentinel_names.insert(dest.clone());
                                stmts.push(Stmt::Assign {
                                    lhs: dest.clone(),
                                    rhs: "__canon_suppressed__".to_string(),
                                });
                            }
                        }
                        continue;
                    }
                    if stmt_defines_ret(&struct_stmt) {
                        ret_value_defined = true;
                        ret_binding_emitted = true;
                    }
                    if let Stmt::StructLit { dest: Some(dest), .. } = &struct_stmt {
                        defined.insert(dest.clone());
                    }
                    stmts.push(struct_stmt);
                }
                continue;
            }
            if is_opaque_aggregate_candidate(rvalue) {
                if let Some(lhs_name) = lhs_name.clone() {
                    defined.insert(lhs_name.clone());
                    if lhs_name == "__ret" {
                        ret_value_defined = true;
                        if !match_dest_emitted {
                            stmts.push(Stmt::Match {
                                dest: Some("__ret".to_string()),
                            });
                            match_dest_emitted = true;
                            ret_binding_emitted = true;
                        }
                    }
                }
                continue;
            }
            if is_zero_arg_enum_ctor_use(tcx, rvalue) {
                if let Some(lhs_name) = lhs_name.clone() {
                    let is_ret = lhs_name == "__ret";
                    defined.insert(lhs_name.clone());
                    suppressed_sentinel_names.insert(lhs_name.clone());
                    stmts.push(Stmt::Assign {
                        lhs: lhs_name,
                        rhs: "__canon_suppressed__".to_string(),
                    });
                    if is_ret {
                        ret_value_defined = true;
                        ret_binding_emitted = true;
                    }
                }
                continue;
            }
            if let Some(assign_stmt) = mir_assign_stmt(
                tcx,
                &body.local_decls,
                lhs,
                rvalue,
                &resolver,
                &defined,
                &suppressed_sentinel_names,
            ) {
                if !stmt_inputs_known(&assign_stmt, &defined, &suppressed_sentinel_names) {
                    if let Stmt::Assign { lhs, .. } = &assign_stmt {
                        if lhs != "__ret" && !defined.contains(lhs) {
                            defined.insert(lhs.clone());
                            suppressed_sentinel_names.insert(lhs.clone());
                            stmts.push(Stmt::Assign {
                                lhs: lhs.clone(),
                                rhs: "__canon_suppressed__".to_string(),
                            });
                        }
                    }
                    continue;
                }
                if stmt_defines_ret(&assign_stmt) {
                    ret_value_defined = true;
                    ret_binding_emitted = true;
                }
                if let Stmt::Assign { lhs, .. } = &assign_stmt {
                    defined.insert(lhs.clone());
                }
                stmts.push(assign_stmt);
            } else if let Some(lhs_name) = lhs_name.clone() {
                if lhs_name != "__ret" && !defined.contains(&lhs_name) {
                    defined.insert(lhs_name.clone());
                    suppressed_sentinel_names.insert(lhs_name.clone());
                    stmts.push(Stmt::Assign {
                        lhs: lhs_name,
                        rhs: "__canon_suppressed__".to_string(),
                    });
                }
            }
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
                if filtered_internal_call_target(tcx, func, args, &resolver) {
                    if let Some(dest) = label_place_dest(&resolver, destination) {
                        if dest == "__ret" {
                            ret_value_defined = true;
                            ret_binding_emitted = true;
                        }
                        defined.insert(dest.clone());
                        suppressed_sentinel_names.insert(dest.clone());
                        stmts.push(Stmt::Assign {
                            lhs: dest,
                            rhs: "__canon_suppressed__".to_string(),
                        });
                    }
                } else if let Some(method_stmt) = mir_method_call_stmt(tcx, &func, &args, &destination, &resolver) {
                    if !stmt_inputs_known(&method_stmt, &defined, &suppressed_sentinel_names) {
                        if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
                            if dest != "__ret" && !defined.contains(dest) {
                                defined.insert(dest.clone());
                                suppressed_sentinel_names.insert(dest.clone());
                                stmts.push(Stmt::Assign {
                                    lhs: dest.clone(),
                                    rhs: "__canon_suppressed__".to_string(),
                                });
                            }
                        }
                        term = target
                            .and_then(|bb| remap_bb_target(bb, &mir_to_emitted))
                            .map(Terminator::Goto)
                            .unwrap_or(Terminator::None);
                    } else {
                        if stmt_defines_ret(&method_stmt) {
                            ret_value_defined = true;
                            ret_binding_emitted = true;
                        }
                        if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
                            defined.insert(dest.clone());
                        }
                        stmts.push(method_stmt);
                    }
                } else if let Some(call_stmt) = mir_call_stmt(tcx, &func, &args, &destination, &resolver) {
                    if stmt_inputs_known(&call_stmt, &defined, &suppressed_sentinel_names) {
                        if stmt_defines_ret(&call_stmt) {
                            ret_value_defined = true;
                            ret_binding_emitted = true;
                        }
                        if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
                            defined.insert(dest.clone());
                        }
                        stmts.push(call_stmt);
                    } else if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
                        if dest != "__ret" && !defined.contains(dest) {
                            defined.insert(dest.clone());
                            suppressed_sentinel_names.insert(dest.clone());
                            stmts.push(Stmt::Assign {
                                lhs: dest.clone(),
                                rhs: "__canon_suppressed__".to_string(),
                            });
                        }
                    }
                } else if let Some(dest_name) = label_place_dest(&resolver, destination) {
                    if dest_name != "__ret" {
                        defined.insert(dest_name.clone());
                        suppressed_sentinel_names.insert(dest_name.clone());
                        stmts.push(Stmt::Assign {
                            lhs: dest_name,
                            rhs: "__canon_suppressed__".to_string(),
                        });
                    } else {
                        if !match_dest_emitted {
                            stmts.push(Stmt::Match {
                                dest: Some("__ret".to_string()),
                            });
                            match_dest_emitted = true;
                            ret_value_defined = true;
                            ret_binding_emitted = true;
                        }
                        defined.insert("__ret".to_string());
                    }
                }
                term = target
                    .and_then(|bb| remap_bb_target(bb, &mir_to_emitted))
                    .map(Terminator::Goto)
                    .unwrap_or(Terminator::None);
            } else if matches!(term_ref.kind, mir::TerminatorKind::Return) {
                if returns_unit {
                    stmts.push(Stmt::Return(None));
                } else if ret_binding_emitted && !match_dest_emitted {
                    stmts.push(Stmt::Return(Some("__ret".to_string())));
                } else if !ret_value_defined && !match_dest_emitted {
                    stmts.push(Stmt::Match {
                        dest: Some("__ret".to_string()),
                    });
                    match_dest_emitted = true;
                    ret_value_defined = true;
                    ret_binding_emitted = true;
                    defined.insert("__ret".to_string());
                } else if !match_dest_emitted && !ret_binding_emitted {
                    stmts.push(Stmt::Match {
                        dest: Some("__ret".to_string()),
                    });
                    match_dest_emitted = true;
                    ret_value_defined = true;
                    ret_binding_emitted = true;
                    defined.insert("__ret".to_string());
                }
                term = Terminator::None;
            } else if let mir::TerminatorKind::Goto { target } = term_ref.kind {
                term = remap_bb_target(target, &mir_to_emitted)
                    .map(Terminator::Goto)
                    .unwrap_or(Terminator::None);
            } else if let mir::TerminatorKind::Drop { target, .. } = term_ref.kind {
                term = remap_bb_target(target, &mir_to_emitted)
                    .map(Terminator::Goto)
                    .unwrap_or(Terminator::None);
            } else if let mir::TerminatorKind::Assert { target, .. } = term_ref.kind {
                term = remap_bb_target(target, &mir_to_emitted)
                    .map(Terminator::Goto)
                    .unwrap_or(Terminator::None);
            } else if let mir::TerminatorKind::SwitchInt { discr, .. } = &term_ref.kind {
                let mut succ = term_ref.successors();
                if let (Some(t), Some(f)) = (succ.next(), succ.next()) {
                    if let Some(cond) = mir_operand_label(tcx, &discr, &resolver) {
                        let true_bb = remap_bb_target(t, &mir_to_emitted);
                        let false_bb = remap_bb_target(f, &mir_to_emitted);
                        term = match (true_bb, false_bb) {
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
            }
        }

        blocks.push(BasicBlock { stmts, terminator: term });
    }

    Body::Blocks(blocks)
}

fn remap_bb_target(target: mir::BasicBlock, mir_to_emitted: &[Option<u32>]) -> Option<u32> {
    mir_to_emitted.get(target.as_usize()).and_then(|slot| *slot)
}

fn bb_writes_return_place(bb: &mir::BasicBlockData<'_>) -> bool {
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

fn stmt_defines_ret(stmt: &Stmt) -> bool {
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

fn value_known(value: &str, defined: &HashSet<String>, suppressed_sentinel_names: &HashSet<String>) -> bool {
    if expr_uses_suppressed_sentinel(value, suppressed_sentinel_names) {
        return false;
    }
    if suppressed_sentinel_names.contains(value) {
        return false;
    }
    if is_synthetic_name(value) {
        return false;
    }
    defined.contains(value) || value == "__ret" || is_structural_expr(value)
}

fn expr_uses_suppressed_sentinel(value: &str, suppressed_sentinel_names: &HashSet<String>) -> bool {
    value
        .split(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
        .any(|tok| !tok.is_empty() && suppressed_sentinel_names.contains(tok))
}

fn is_synthetic_name(s: &str) -> bool {
    let s = s.strip_prefix('_').unwrap_or(s);
    let Some(rest) = s.strip_prefix('v') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn is_structural_expr(value: &str) -> bool {
    value.contains("::")
        || value.starts_with('*')
        || value.contains('(')
        || value.contains(')')
        || value.contains('[')
        || value.contains(']')
        || value.contains('&')
        || value.contains(' ')
        || value.starts_with('"')
        || value.starts_with('\'')
        || value == "true"
        || value == "false"
        || value.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-')
}

fn stmt_inputs_known(
    stmt: &Stmt,
    defined: &HashSet<String>,
    suppressed_sentinel_names: &HashSet<String>,
) -> bool {
    match stmt {
        Stmt::Assign { rhs, .. } => value_known(rhs, defined, suppressed_sentinel_names),
        Stmt::Call { args, .. } => args
            .iter()
            .all(|a| value_known(a, defined, suppressed_sentinel_names)),
        Stmt::FieldAccess { base, .. } => value_known(base, defined, suppressed_sentinel_names),
        Stmt::MethodCall { receiver, args, .. } => {
            value_known(receiver, defined, suppressed_sentinel_names)
                && args
                    .iter()
                    .all(|a| value_known(a, defined, suppressed_sentinel_names))
        }
        Stmt::StructLit { fields, .. } => fields
            .iter()
            .all(|(_, v)| value_known(v, defined, suppressed_sentinel_names)),
        Stmt::Match { .. } => true,
        _ => true,
    }
}

fn is_field_access_candidate(rvalue: &mir::Rvalue<'_>) -> bool {
    matches!(
        rvalue,
        mir::Rvalue::Use(mir::Operand::Copy(place) | mir::Operand::Move(place))
            if matches!(place.as_ref().last_projection(), Some((_, mir::ProjectionElem::Field(..))))
    )
}

fn is_struct_lit_candidate(rvalue: &mir::Rvalue<'_>) -> bool {
    matches!(rvalue, mir::Rvalue::Aggregate(kind, _) if matches!(&**kind, mir::AggregateKind::Adt(_, _, _, _, _)))
}

fn is_opaque_aggregate_candidate(rvalue: &mir::Rvalue<'_>) -> bool {
    matches!(
        rvalue,
        mir::Rvalue::Aggregate(kind, _)
            if matches!(
                &**kind,
                mir::AggregateKind::Closure(_, _)
                    | mir::AggregateKind::Coroutine(_, _)
                    | mir::AggregateKind::CoroutineClosure(_, _)
            )
    )
}

fn is_method_call_candidate(tcx: TyCtxt<'_>, func: &mir::Operand<'_>) -> bool {
    func.const_fn_def().map(|(did, _)| matches!(tcx.def_kind(did), DefKind::AssocFn)).unwrap_or(false)
}

fn mir_assign_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    local_decls: &mir::LocalDecls<'tcx>,
    lhs: &mir::Place<'tcx>,
    rvalue: &mir::Rvalue<'tcx>,
    resolver: &LocalNameResolver,
    defined: &HashSet<String>,
    suppressed_sentinel_names: &HashSet<String>,
) -> Option<Stmt> {
    if is_zero_arg_enum_ctor_use(tcx, rvalue) {
        return None;
    }
    let lhs = resolver.label_place(lhs)?;
    if matches!(rvalue, mir::Rvalue::Aggregate(kind, _) if matches!(&**kind, mir::AggregateKind::Array(_)))
        && defined.contains(&lhs)
    {
        return None;
    }
    let rhs = mir_rvalue_expr(tcx, local_decls, rvalue, resolver)?;
    if is_zero_arg_enum_ctor_expr_str(&rhs) {
        return None;
    }
    if rhs.contains("SizedTypeProperties") {
        return None;
    }
    if lhs == "__ret" {
        return Some(Stmt::Assign { lhs, rhs });
    }
    if !value_known(&rhs, defined, suppressed_sentinel_names) {
        return None;
    }
    Some(Stmt::Assign { lhs, rhs })
}

fn is_zero_arg_enum_ctor_expr_str(expr: &str) -> bool {
    let expr = strip_instance_generics(expr);
    expr == "std::option::Option::None"
        || expr == "core::option::Option::None"
        || expr == "Option::None"
}

fn is_zero_arg_enum_ctor_use(tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
    let mir::Rvalue::Use(mir::Operand::Constant(c)) = rvalue else {
        return false;
    };
    if let ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
        if matches!(
            tcx.def_kind(*did),
            DefKind::Ctor(rustc_hir::def::CtorOf::Variant, rustc_hir::def::CtorKind::Const)
        ) {
            return true;
        }
    }
    let ty::TyKind::Adt(adt, _) = c.const_.ty().kind() else {
        return false;
    };
    if !adt.is_enum() {
        return false;
    }
    if let mir::Const::Val(v, ty) = c.const_ {
        if v.try_to_scalar_int().is_some()
            && matches!(ty.kind(), ty::TyKind::Adt(adt2, _) if adt2.is_enum())
            && adt.variants().iter().any(|var| var.fields.is_empty())
        {
            return true;
        }
    }
    let rendered = strip_instance_generics(&c.const_.to_string());
    for variant in adt.variants().iter() {
        if !variant.fields.is_empty() {
            continue;
        }
        let suffix = format!("::{}", variant.name);
        if rendered.ends_with(&suffix) {
            return true;
        }
    }
    false
}

fn mir_rvalue_expr<'tcx>(
    tcx: TyCtxt<'tcx>,
    local_decls: &mir::LocalDecls<'tcx>,
    rvalue: &mir::Rvalue<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<String> {
    match rvalue {
        mir::Rvalue::Use(op) => match op {
            mir::Operand::Copy(place) | mir::Operand::Move(place) => {
                if place.projection.is_empty() {
                    resolver.label_place(place)
                } else {
                    render_projected_place_expr(tcx, local_decls, place, resolver)
                }
            }
            _ => mir_operand_label(tcx, op, resolver),
        },
        mir::Rvalue::Ref(_, borrow_kind, place) => {
            let place = render_projected_place_expr(tcx, local_decls, place, resolver)?;
            Some(match borrow_kind {
                mir::BorrowKind::Mut { .. } => format!("&mut {place}"),
                _ => format!("&{place}"),
            })
        }
        mir::Rvalue::RawPtr(raw_ptr_kind, place) => {
            let place = render_projected_place_expr(tcx, local_decls, place, resolver)?;
            Some(if matches!(raw_ptr_kind, mir::RawPtrKind::Mut) {
                format!("&mut {place}")
            } else {
                format!("&{place}")
            })
        }
        mir::Rvalue::BinaryOp(op, boxed) => {
            let (lhs, rhs) = &**boxed;
            Some(format!(
                "({} {} {})",
                mir_operand_label(tcx, lhs, resolver)?,
                mir_binop_token(*op)?,
                mir_operand_label(tcx, rhs, resolver)?,
            ))
        }
        mir::Rvalue::UnaryOp(op, operand) => {
            Some(format!("({}{})", mir_unop_token(*op), mir_operand_label(tcx, operand, resolver)?))
        }
        mir::Rvalue::Cast(_, operand, ty) => Some(format!(
            "({} as {})",
            mir_operand_label(tcx, operand, resolver)?,
            render_type_expr(tcx, &lower_ty(tcx, *ty))
        )),
        mir::Rvalue::Aggregate(kind, operands) => match &**kind {
            mir::AggregateKind::Tuple => {
                let elems = operands
                    .iter()
                    .map(|op| mir_operand_label(tcx, op, resolver))
                    .collect::<Option<Vec<_>>>()?;
                if elems.len() == 1 {
                    Some(format!("({},)", elems[0]))
                } else {
                    Some(format!("({})", elems.join(", ")))
                }
            }
            mir::AggregateKind::Array(_) => {
                let elems = operands
                    .iter()
                    .map(|op| mir_operand_label(tcx, op, resolver))
                    .collect::<Option<Vec<_>>>()?;
                Some(format!("[{}]", elems.join(", ")))
            }
            _ => None,
        },
        mir::Rvalue::Repeat(operand, count) => {
            let count = count.try_to_target_usize(tcx)?;
            Some(format!("[{}; {count}]", mir_operand_label(tcx, operand, resolver)?))
        }
        mir::Rvalue::Discriminant(place) => Some(format!("{} as isize", resolver.label_place(place)?)),
        mir::Rvalue::CopyForDeref(place) => resolver.label_place(place),
        _ => None,
    }
}

fn mir_binop_token(op: mir::BinOp) -> Option<&'static str> {
    match op {
        mir::BinOp::Add => Some("+"),
        mir::BinOp::Sub => Some("-"),
        mir::BinOp::Mul => Some("*"),
        mir::BinOp::Div => Some("/"),
        mir::BinOp::Rem => Some("%"),
        mir::BinOp::BitXor => Some("^"),
        mir::BinOp::BitAnd => Some("&"),
        mir::BinOp::BitOr => Some("|"),
        mir::BinOp::Shl => Some("<<"),
        mir::BinOp::Shr => Some(">>"),
        mir::BinOp::Eq => Some("=="),
        mir::BinOp::Lt => Some("<"),
        mir::BinOp::Le => Some("<="),
        mir::BinOp::Ne => Some("!="),
        mir::BinOp::Ge => Some(">="),
        mir::BinOp::Gt => Some(">"),
        mir::BinOp::Cmp => None,
        mir::BinOp::Offset => None,
        mir::BinOp::AddUnchecked => Some("+"),
        mir::BinOp::SubUnchecked => Some("-"),
        mir::BinOp::MulUnchecked => Some("*"),
        mir::BinOp::ShlUnchecked => Some("<<"),
        mir::BinOp::ShrUnchecked => Some(">>"),
        mir::BinOp::AddWithOverflow => Some("+"),
        mir::BinOp::SubWithOverflow => Some("-"),
        mir::BinOp::MulWithOverflow => Some("*"),
    }
}

fn mir_unop_token(op: mir::UnOp) -> &'static str {
    match op {
        mir::UnOp::Not => "!",
        mir::UnOp::Neg => "-",
        mir::UnOp::PtrMetadata => "",
    }
}

fn mir_field_access_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    local_decls: &mir::LocalDecls<'tcx>,
    lhs: &mir::Place<'tcx>,
    rvalue: &mir::Rvalue<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<Stmt> {
    let mir::Rvalue::Use(mir::Operand::Copy(place) | mir::Operand::Move(place)) = rvalue else {
        return None;
    };
    let (base, proj) = place.as_ref().last_projection()?;
    let mir::ProjectionElem::Field(field_idx, ty) = proj else {
        return None;
    };

    let base_ty = base.ty(local_decls, tcx).ty;
    if is_primitive_value_ty(base_ty) {
        return None;
    }

    let field = match base_ty.kind() {
        ty::TyKind::Adt(adt, _) if adt.is_struct() || adt.is_union() => {
            let f = adt.non_enum_variant().fields.get(field_idx)?;
            let name = f.name.to_string();
            if name.chars().all(|c| c.is_ascii_digit()) {
                field_idx.index().to_string()
            } else {
                name
            }
        }
        ty::TyKind::Adt(adt, _) if adt.is_enum() => {
            let downcast_idx = place
                .projection
                .iter()
                .find_map(|elem| match elem {
                    mir::ProjectionElem::Downcast(_, idx) => Some(idx),
                    _ => None,
                })?;
            let f = adt.variant(downcast_idx).fields.get(field_idx)?;
            let name = f.name.to_string();
            if name.chars().all(|c| c.is_ascii_digit()) {
                field_idx.index().to_string()
            } else {
                name
            }
        }
        ty::TyKind::Tuple(_) => field_idx.index().to_string(),
        _ => return None,
    };
    Some(Stmt::FieldAccess {
        base: resolver.label_place_ref(base)?,
        field,
        dest: Some(resolver.label_place(lhs)?),
    })
}

fn is_primitive_value_ty(ty: ty::Ty<'_>) -> bool {
    matches!(
        ty.kind(),
        ty::TyKind::Bool
            | ty::TyKind::Char
            | ty::TyKind::Int(..)
            | ty::TyKind::Uint(..)
            | ty::TyKind::Float(..)
            | ty::TyKind::Str
            | ty::TyKind::Never
    )
}

fn mir_struct_lit_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    lhs: &mir::Place<'tcx>,
    rvalue: &mir::Rvalue<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<Stmt> {
    let mir::Rvalue::Aggregate(kind, operands) = rvalue else {
        return None;
    };
    let mir::AggregateKind::Adt(adt_did, variant_idx, _, _, _) = &**kind else {
        return None;
    };
    let adt = tcx.adt_def(*adt_did);
    let variant = adt.variant(*variant_idx);
    if adt.is_enum() && variant.fields.is_empty() {
        return None;
    }
    let fields = variant
        .fields
        .iter()
        .zip(operands.iter())
        .map(|(f, op)| Some((f.name.to_string(), mir_operand_label(tcx, op, resolver)?)))
        .collect::<Option<Vec<_>>>()?;
    let ctor_path = if adt.is_enum() {
        format!("{}::{}", norm::path(tcx, *adt_did), variant.name)
    } else {
        norm::path(tcx, *adt_did)
    };
    Some(Stmt::StructLit {
        ty: TypeExpr::Path(ctor_path),
        fields,
        dest: Some(resolver.label_place(lhs)?),
    })
}

fn mir_method_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    destination: &mir::Place<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<Stmt> {
    let (did, _) = func.const_fn_def()?;
    if !matches!(tcx.def_kind(did), DefKind::AssocFn) || args.is_empty() {
        return None;
    }
    let receiver = match mir_operand_label_for_arg(tcx, &args[0].node, resolver)? {
        ArgLabel::Value(v) => v,
        ArgLabel::Omit => return None,
    };
    let method = tcx.item_name(did).to_string();
    let args = mir_call_args_labels(tcx, &args[1..], resolver)?;
    Some(Stmt::MethodCall {
        receiver,
        method,
        args,
        dest: Some(label_place_dest(resolver, destination)?),
    })
}

fn mir_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    destination: &mir::Place<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<Stmt> {
    let func = if let Some((did, _)) = func.const_fn_def() {
        norm::path(tcx, did)
    } else {
        mir_operand_label(tcx, func, resolver)?
    };
    let args = mir_call_args_labels(tcx, args, resolver)?;
    Some(Stmt::Call {
        func,
        args,
        dest: Some(label_place_dest(resolver, destination)?),
    })
}

fn mir_call_args_labels<'tcx>(
    tcx: TyCtxt<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    resolver: &LocalNameResolver,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match mir_operand_label_for_arg(tcx, &arg.node, resolver)? {
            ArgLabel::Value(v) => out.push(v),
            ArgLabel::Omit => {}
        }
    }
    Some(out)
}

enum ArgLabel {
    Value(String),
    Omit,
}

fn mir_operand_label_for_arg(tcx: TyCtxt<'_>, operand: &mir::Operand<'_>, resolver: &LocalNameResolver) -> Option<ArgLabel> {
    match operand {
        mir::Operand::Constant(c) if constant_is_implicit_zst_value(c) => Some(ArgLabel::Omit),
        _ => mir_operand_label(tcx, operand, resolver).map(ArgLabel::Value),
    }
}

fn constant_is_implicit_zst_value(constant: &mir::ConstOperand<'_>) -> bool {
    matches!(
        constant.const_.ty().kind(),
        ty::TyKind::FnDef(..)
            | ty::TyKind::Closure(..)
            | ty::TyKind::Coroutine(..)
            | ty::TyKind::CoroutineClosure(..)
    )
}

fn mir_operand_label(tcx: TyCtxt<'_>, operand: &mir::Operand<'_>, resolver: &LocalNameResolver) -> Option<String> {
    match operand {
        mir::Operand::Copy(place) | mir::Operand::Move(place) => resolver.label_place(place),
        mir::Operand::Constant(c) => {
            if let ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
                return Some(norm::path(tcx, *did));
            }
            if let mir::Const::Unevaluated(uneval, _) = c.const_ {
                Some(norm::path(tcx, uneval.def))
            } else {
                let const_str = c.const_.to_string();
                if const_str.is_empty() || const_str == "_" {
                    None
                } else {
                    Some(strip_instance_generics(&const_str))
                }
            }
        }
        mir::Operand::RuntimeChecks(_) => None,
    }
}

fn label_place_dest(
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

fn strip_instance_generics(raw: &str) -> String {
    if !raw.contains("::<") {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' && chars[i + 2] == '<' {
            i += 3;
            let mut depth = 1usize;
            while i < chars.len() && depth > 0 {
                match chars[i] {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

struct LocalNameResolver {
    by_local: HashMap<u32, String>,
}

impl LocalNameResolver {
    fn new<'tcx>(body: &mir::Body<'tcx>, param_names: &[String]) -> Self {
        let mut by_local: HashMap<u32, String> = HashMap::new();
        by_local.insert(0, "__ret".to_string());
        for (idx, name) in param_names.iter().enumerate() {
            let local_idx = (idx + 1) as u32;
            if is_rust_ident(name) {
                by_local.insert(local_idx, name.clone());
            }
        }
        for dbg in &body.var_debug_info {
            let mir::VarDebugInfoContents::Place(place) = &dbg.value else {
                continue;
            };
            let projection_ok = place.projection.is_empty()
                || (place.projection.len() == 1
                    && matches!(
                        place.projection[0],
                        mir::ProjectionElem::Field(..) | mir::ProjectionElem::Deref
                    ));
            if !projection_ok {
                continue;
            }
            let name = dbg.name.as_str().to_string();
            if !is_rust_ident(&name) {
                continue;
            }
            by_local.entry(place.local.as_u32()).or_insert(name);
        }
        for local in body.local_decls.indices() {
            by_local.entry(local.as_u32()).or_insert_with(|| format!("_v{}", local.as_u32()));
        }
        Self { by_local }
    }

    fn label_place(&self, place: &mir::Place<'_>) -> Option<String> {
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::Downcast(..)))
        {
            return None;
        }
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..)))
        {
            return None;
        }
        if !place.projection.is_empty() {
            return None;
        }
        self.label_local(place.local)
    }

    fn label_local(&self, local: mir::Local) -> Option<String> {
        let name = self.by_local.get(&local.as_u32())?;
        if !is_value_name_safe(name) {
            return None;
        }
        Some(name.clone())
    }

    fn label_place_ref(&self, place: mir::PlaceRef<'_>) -> Option<String> {
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::Downcast(..)))
        {
            return None;
        }
        if place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..)))
        {
            return None;
        }
        if !place.projection.is_empty() {
            return None;
        }
        let name = self.by_local.get(&place.local.as_u32())?;
        if !is_value_name_safe(name) {
            return None;
        }
        Some(name.clone())
    }
}

fn render_projected_place_expr<'tcx>(
    tcx: TyCtxt<'tcx>,
    local_decls: &mir::LocalDecls<'tcx>,
    place: &mir::Place<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<String> {
    if place.projection.is_empty() {
        return resolver.label_place(place);
    }
    let mut expr = resolver.label_local(place.local)?;
    let mut cursor_ty = local_decls[place.local].ty;
    let mut pending_downcast: Option<String> = None;
    for elem in place.projection.iter() {
        match elem {
            mir::ProjectionElem::Deref => {
                expr = format!("*{expr}");
                cursor_ty = cursor_ty.builtin_deref(true)?;
            }
            mir::ProjectionElem::Downcast(variant_name, variant_idx) => {
                let variant = variant_name
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| format!("variant_{}", variant_idx.as_usize()));
                pending_downcast = Some(variant);
            }
            mir::ProjectionElem::Field(field_idx, field_ty) => {
                let field = if let Some(variant) = pending_downcast.take() {
                    expr = format!("({expr} as {variant})");
                    field_idx.index().to_string()
                } else {
                    match cursor_ty.kind() {
                        ty::TyKind::Adt(adt, _) => {
                            let f = adt.non_enum_variant().fields.get(field_idx)?;
                            let name = f.name.to_string();
                            if name.chars().all(|c| c.is_ascii_digit()) {
                                field_idx.index().to_string()
                            } else {
                                name
                            }
                        }
                        ty::TyKind::Tuple(_) => field_idx.index().to_string(),
                        _ => field_idx.index().to_string(),
                    }
                };
                expr = format!("({expr}).{field}");
                cursor_ty = field_ty;
            }
            mir::ProjectionElem::Index(local) => {
                let idx = resolver.label_local(local)?;
                expr = format!("{expr}[{idx}]");
                cursor_ty = match cursor_ty.kind() {
                    ty::TyKind::Array(inner, _) | ty::TyKind::Slice(inner) => *inner,
                    _ => cursor_ty,
                };
            }
            mir::ProjectionElem::OpaqueCast(..) | mir::ProjectionElem::UnwrapUnsafeBinder(..) => {}
            _ => return None,
        }
    }
    if let Some(variant) = pending_downcast {
        expr = format!("({expr} as {variant})");
    }
    Some(expr)
}

fn is_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_value_name_safe(s: &str) -> bool {
    if s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return false;
    }
    true
}

fn is_filtered_internal_call_path(path: &str) -> bool {
    matches!(
        path,
        "std::hint::must_use"
            | "core::hint::must_use"
            | "std::io::_print"
            | "std::io::_eprint"
            | "core::fmt::Arguments::new_v1"
            | "std::fmt::Arguments::new_v1"
            | "core::fmt::Arguments::new_v1_formatted"
            | "std::fmt::Arguments::new_v1_formatted"
    ) || path.ends_with("::new_display")
        || path.ends_with("::branch")
        || path.ends_with("::from_residual")
        || path.ends_with("::from_output")
        || path.ends_with("::from_str")
        || path.contains("SizedTypeProperties")
        || path.contains("::__iterator_get_unchecked")
        || path.ends_with("::is_val_statically_known")
        || path.ends_with("::parse")
        || path.ends_with("::into")
        || path.ends_with("::new")
}

fn filtered_internal_call_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    _args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    _resolver: &LocalNameResolver,
) -> bool {
    let Some((did, _)) = func.const_fn_def() else {
        return false;
    };
    let path = norm::path(tcx, did);
    if is_filtered_internal_call_path(&path) {
        return true;
    }
    if path_has_unresolved_generic(&path) {
        return true;
    }
    false
}

fn path_has_unresolved_generic(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 2] == b'>' && bytes[i + 1].is_ascii_uppercase() {
            return true;
        }
        i += 1;
    }
    false
}

fn count_local_uses<'tcx>(body: &mir::Body<'tcx>) -> HashMap<u32, usize> {
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
