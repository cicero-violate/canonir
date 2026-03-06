use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;
use std::collections::HashSet;
// structural fallback adjustments applied

use crate::capture::mir::analysis::SwitchAnalysis;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::capture::mir::util as mir_util;
use crate::types::{Stmt, Terminator};

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, func: &mir::Operand<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], destination: &mir::Place<'tcx>,
    target: Option<mir::BasicBlock>, resolver: &LocalNameResolver, mir_to_emitted: &[Option<u32>], stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>,
    suppressed_sentinel_names: &mut HashSet<String>, has_match_dest: bool,
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
            panic!("canon-capture invariant violation: unsupported deref call lowering");
        }
    } else if must_use_call && let Some(dest) = mir_util::label_place_dest(resolver, destination) {
        if let Some(arg) = args.first()
            && let Some(arg_value) = mir_ops::mir_operand_label(tcx, &arg.node, resolver)
        {
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: arg_value });
            defined.insert(dest);
        } else {
            panic!("canon-capture invariant violation: unsupported must_use call lowering");
        }
    } else if let Some(dest) = mir_util::label_place_dest(resolver, destination)
        && let Some(method_stmt) = mir_ops::mir_method_call_stmt(tcx, local_decls, func, args, resolver, dest.clone())
    {
        if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
            defined.insert(dest.clone());
        }
        stmts.push(method_stmt);
    } else if let Some(dest) = mir_util::label_place_dest(resolver, destination)
        && let Some(call_stmt) = mir_ops::mir_call_stmt(tcx, local_decls, func, args, resolver, dest.clone())
    {
        if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
            defined.insert(dest.clone());
        }
        stmts.push(call_stmt);
    } else if let Some(dest) = mir_util::label_place_dest(resolver, destination) {
        // Structural fallback: emit a direct call expression assignment if possible.
        if let Some(func_label) = mir_ops::mir_operand_label(tcx, func, resolver)
            && let Some(arg_labels) = mir_call_args_labels_fallback(tcx, local_decls, args, resolver)
        {
            let call_expr = format!("{}({})", func_label, arg_labels.join(", "));
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: call_expr });
            defined.insert(dest);
        } else {
            // Last-resort structural fallback: use a type-directed diverging panic
            // so the destination retains authoritative MIR type.
            let local = destination.local;
            let ty = local_decls[local].ty;
            let lowered = crate::capture::helpers::lower_ty(tcx, ty);
            let ty_expr = crate::capture::helpers::render_type_expr(tcx, &lowered);

            // Fail fast on private fmt internals instead of emitting casts
            // that reference unstable core internals. Do not attempt to
            // fabricate a typed fallback for these compiler-private types.
            if ty_expr.contains("fmt::rt::Argument") {
                panic!("canon invariant violation: unsupported call lowering for private fmt internal type `{}`", ty_expr);
            }

            panic!("canon-capture invariant violation: unresolved call lowering func={func:?} args={args:?} destination={destination:?}");
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
    has_ret_binding: bool, has_match_dest: bool, mir_idx: usize, switch_analysis: &SwitchAnalysis,
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
            if let Some(body_entry) = switch_analysis.iterator_switches.get(&mir_idx).copied() {
                let target = mir::BasicBlock::from_usize(body_entry);
                return remap_to_goto(target, mir_to_emitted);
            }
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

fn mir_call_args_labels_fallback<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], resolver: &LocalNameResolver,
) -> Option<Vec<String>> {
    if let Some(labels) = mir_ops::mir_call_args_labels(tcx, args, resolver, local_decls) {
        return Some(labels);
    }

    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match &arg.node {
            mir::Operand::Copy(place) | mir::Operand::Move(place) => {
                if let Some(label) = mir_ops::mir_operand_label_with_decls(tcx, &arg.node, resolver, Some(local_decls)) {
                    out.push(label);
                    continue;
                }
                if let Some(name) = resolver.label_local(place.local) {
                    out.push(name);
                    continue;
                }
                return None;
            }
            _ => {
                let label = mir_ops::mir_operand_label_with_decls(tcx, &arg.node, resolver, Some(local_decls))?;
                out.push(label);
            }
        }
    }
    Some(out)
}

fn remap_to_goto(target: mir::BasicBlock, mir_to_emitted: &[Option<u32>]) -> Terminator {
    mir_util::remap_bb_target(target, mir_to_emitted).map(Terminator::Goto).unwrap_or(Terminator::None)
}

fn lower_return_terminator(returns_unit: bool, stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>, _has_ret_binding: bool, _has_match_dest: bool) {
    if returns_unit {
        stmts.push(Stmt::Return(None));
    } else {
        // RETURN INTEGRITY:
        // The MIR return place (local 0) must be lowered as a concrete
        // assignment to "__ret" before any Return terminator is emitted.
        // If it was not, this is a structural lowering defect and must
        // fail fast rather than fabricating a value or emitting an
        // untyped return that breaks downstream type determinism.
        if !defined.contains("__ret") {
            panic!("canon-capture invariant violation: non-unit function returning without __ret binding");
        }
        // Emit a structural return of the authoritative __ret binding.
        // No fabrication or fallback value is permitted here.
        // Do not blindly dereference at the return boundary; type
        // normalization must be handled during assignment/rvalue lowering.
        stmts.push(Stmt::Return(Some("__ret".to_string())));
    }
}
