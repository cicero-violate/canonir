use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use std::collections::HashSet;

use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::capture::mir::util as mir_util;
use crate::types::{Stmt, Terminator};

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, func: &mir::Operand<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], destination: &mir::Place<'tcx>, target: Option<mir::BasicBlock>,
    resolver: &LocalNameResolver, mir_to_emitted: &[Option<u32>], stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>, suppressed_sentinel_names: &mut HashSet<String>, has_match_dest: bool,
) -> Terminator {
    let must_use_call =
        mir_ops::call_target_path(tcx, func).map(|p| p.contains("must_use")).unwrap_or(false) || mir_ops::mir_operand_label(tcx, func, resolver).map(|f| f.ends_with("must_use")).unwrap_or(false);

    if mir_ops::is_deref_call_target(tcx, func, resolver)
        && let Some(dest) = mir_util::label_place_dest(resolver, destination)
    {
        if let Some(arg) = args.first()
            && let Some(arg_value) = mir_ops::mir_operand_label(tcx, &arg.node, resolver)
        {
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: arg_value });
            defined.insert(dest);
        } else {
            mir_util::emit_suppressed_for_name(&dest, stmts, defined, suppressed_sentinel_names);
        }
    } else if must_use_call && let Some(dest) = mir_util::label_place_dest(resolver, destination) {
        if let Some(arg) = args.first()
            && let Some(arg_value) = mir_ops::mir_operand_label(tcx, &arg.node, resolver)
        {
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: arg_value });
            defined.insert(dest);
        } else if dest == "__ret" {
            defined.insert("__ret".to_string());
        } else {
            mir_util::emit_suppressed_for_name(&dest, stmts, defined, suppressed_sentinel_names);
        }
    } else if mir_ops::is_format_call_target(tcx, func) {
        if let Some(dest) = mir_util::label_place_dest(resolver, destination) {
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: "std::string::String::new()".to_string() });
            defined.insert(dest);
        }
    } else if mir_ops::filtered_internal_call_target(tcx, func, resolver) {
        if let Some(dest) = mir_util::label_place_dest(resolver, destination) {
            if dest != "__ret" {
                mir_util::emit_suppressed_for_name(&dest, stmts, defined, suppressed_sentinel_names);
            }
        }
    } else if let Some(dest) = mir_util::label_place_dest(resolver, destination)
        && let Some(method_stmt) = mir_ops::mir_method_call_stmt(tcx, func, args, resolver, dest.clone())
    {
        if !mir_guard::structural_guard(&method_stmt, defined, suppressed_sentinel_names) {
            if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
                if dest != "__ret" {
                    mir_util::emit_suppressed_for_name(dest, stmts, defined, suppressed_sentinel_names);
                }
            }
            return target.and_then(|bb| mir_util::remap_bb_target(bb, mir_to_emitted)).map(Terminator::Goto).unwrap_or(Terminator::None);
        }
        if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
            defined.insert(dest.clone());
        }
        stmts.push(method_stmt);
    } else if let Some(dest) = mir_util::label_place_dest(resolver, destination)
        && let Some(call_stmt) = mir_ops::mir_call_stmt(tcx, func, args, resolver, dest.clone())
    {
        if mir_guard::structural_guard(&call_stmt, defined, suppressed_sentinel_names) {
            if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
                defined.insert(dest.clone());
            }
            stmts.push(call_stmt);
        } else if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
            if dest != "__ret" {
                mir_util::emit_suppressed_for_name(dest, stmts, defined, suppressed_sentinel_names);
            }
        }
    } else if let Some(dest_name) = mir_util::label_place_dest(resolver, destination) {
        if dest_name != "__ret" {
            mir_guard::emit_suppressed_binding(&dest_name, defined, suppressed_sentinel_names, stmts);
        } else {
            // Do not fabricate or implicitly define __ret here.
            // __ret must only be defined by structurally valid lowering paths.
        }
    }

    target.and_then(|bb| mir_util::remap_bb_target(bb, mir_to_emitted)).map(Terminator::Goto).unwrap_or_else(|| {
        // Do not implicitly fabricate a suppressed __ret here.
        // __ret must be defined by structurally valid lowering paths
        // or by explicit suppressed-ret emission in return handling.
        Terminator::None
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_non_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, term_ref: &mir::Terminator<'tcx>, returns_unit: bool, resolver: &LocalNameResolver, mir_to_emitted: &[Option<u32>], stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>,
    has_ret_binding: bool, has_match_dest: bool,
) -> Terminator {
    match &term_ref.kind {
        mir::TerminatorKind::Return => {
            // Do NOT fabricate __ret here.
            // Deterministic structural fallback will synthesize return if needed.
            lower_return_terminator(returns_unit, stmts, defined, has_ret_binding, has_match_dest);
            Terminator::None
        }
        mir::TerminatorKind::Goto { target } | mir::TerminatorKind::Drop { target, .. } | mir::TerminatorKind::Assert { target, .. } => remap_to_goto(*target, mir_to_emitted),
        mir::TerminatorKind::SwitchInt { discr, .. } => {
            let mut succ = term_ref.successors();
            if let (Some(t), Some(f)) = (succ.next(), succ.next())
                && let Some(cond) = mir_ops::mir_operand_label(tcx, discr, resolver)
            {
                let true_bb = mir_util::remap_bb_target(t, mir_to_emitted);
                let false_bb = mir_util::remap_bb_target(f, mir_to_emitted);
                return match (true_bb, false_bb) {
                    (Some(t), Some(f)) => Terminator::Branch { cond, true_bb: t, false_bb: f },
                    (Some(t), None) => Terminator::Goto(t),
                    (None, Some(f)) => Terminator::Goto(f),
                    (None, None) => Terminator::None,
                };
            }
            Terminator::None
        }
        _ => Terminator::None,
    }
}

fn remap_to_goto(target: mir::BasicBlock, mir_to_emitted: &[Option<u32>]) -> Terminator {
    mir_util::remap_bb_target(target, mir_to_emitted).map(Terminator::Goto).unwrap_or(Terminator::None)
}

fn lower_return_terminator(returns_unit: bool, stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>, has_ret_binding: bool, has_match_dest: bool) {
    if returns_unit {
        stmts.push(Stmt::Return(None));
    } else {
        // Only emit structural return; value must already be lowered
        // from MIR return place. Do not fabricate self-bindings.
        if !defined.contains("__ret") {
            // Deterministically synthesize a concrete return binding
            // instead of leaving a suppressed __ret gap.
            stmts.push(Stmt::Assign { lhs: "__ret".to_string(), rhs: "Default::default()".to_string() });
            defined.insert("__ret".to_string());
        }
        stmts.push(Stmt::Return(Some("__ret".to_string())));
    }
}
