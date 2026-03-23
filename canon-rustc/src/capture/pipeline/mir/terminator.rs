use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};
use std::collections::HashSet;
// structural fallback adjustments applied

use crate::capture::pipeline::mir::analysis::SwitchAnalysis;
use crate::capture::pipeline::mir::expr as mir_expr;
use crate::capture::pipeline::mir::ops as mir_ops;
use crate::capture::pipeline::mir::resolver::LocalNameResolver;
use crate::capture::pipeline::mir::util as mir_util;
use crate::capture::types::{Stmt, Terminator};

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, func: &mir::Operand<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], destination: &mir::Place<'tcx>,
    target: Option<mir::BasicBlock>, resolver: &LocalNameResolver, mir_to_emitted: &[Option<u32>], stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>,
    _suppressed_sentinel_names: &mut HashSet<String>, _has_match_dest: bool,
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
            panic!("lowering error: unsupported deref call lowering");
        }
    } else if must_use_call && let Some(dest) = mir_util::label_place_dest(resolver, destination) {
        if let Some(arg) = args.first()
            && let Some(arg_value) = mir_ops::mir_operand_label(tcx, &arg.node, resolver)
        {
            stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: arg_value });
            defined.insert(dest);
        } else {
            panic!("lowering error: unsupported must_use call lowering");
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
            let lowered = crate::capture::pipeline::helpers::lower_ty(tcx, ty);
            let ty_expr = crate::capture::pipeline::helpers::render_type_expr(tcx, &lowered);

            // Fail fast on private fmt internals instead of emitting casts
            // that reference unstable core internals. Do not attempt to
            // fabricate a typed fallback for these compiler-private types.
            if ty_expr.contains("fmt::rt::Argument") {
                panic!("lowering error: unsupported call lowering for private fmt internal type `{}`", ty_expr);
            }

            panic!("lowering error: unresolved call lowering func={func:?} args={args:?} destination={destination:?}");
        }
    }

    match target {
        None => {
            // Do not implicitly fabricate a suppressed __ret here.
            // __ret must be defined by structurally valid lowering paths
            // or by explicit suppressed-ret emission in return handling.
            Terminator::None
        }
        Some(bb) => {
            let remapped = mir_util::remap_bb_target(bb, mir_to_emitted)
                .unwrap_or_else(|| panic!("lowering error: call terminator target missing target={bb:?}"));
            Terminator::Goto(remapped)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_non_call_terminator<'tcx>(
    tcx: TyCtxt<'tcx>, term_ref: &mir::Terminator<'tcx>, returns_unit: bool, resolver: &LocalNameResolver, mir_to_emitted: &[Option<u32>], stmts: &mut Vec<Stmt>, defined: &mut HashSet<String>,
    has_ret_binding: bool, has_match_dest: bool, mir_idx: usize, switch_analysis: &SwitchAnalysis, local_decls: &mir::LocalDecls<'tcx>,
) -> Terminator {
    let term_span = term_ref.source_info.span;
    let term_file = crate::capture::normalization::file(tcx, term_span);
    let term_span_str = crate::capture::normalization::span(tcx, term_span);
    match &term_ref.kind {
        mir::TerminatorKind::Return => {
            // Do NOT fabricate __ret here.
            // Deterministic structural fallback will synthesize return if needed.
            lower_return_terminator(returns_unit, stmts, has_ret_binding, has_match_dest);
            Terminator::None
        }
        mir::TerminatorKind::Goto { target } | mir::TerminatorKind::Drop { target, .. } | mir::TerminatorKind::Assert { target, .. } => remap_to_goto(*target, mir_to_emitted),
        mir::TerminatorKind::SwitchInt { discr, .. } => {
            if let Some(body_entry) = switch_analysis.iterator_switches.get(&mir_idx).copied() {
                let target = mir::BasicBlock::from_usize(body_entry);
                return remap_to_goto(target, mir_to_emitted);
            }
            let cond = mir_ops::mir_operand_label(tcx, discr, resolver)
                .or_else(|| match discr {
                    mir::Operand::Copy(place) | mir::Operand::Move(place) => {
                        mir_expr::render_projected_place_expr(tcx, local_decls, place, resolver)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "lowering error: SwitchInt discriminant unresolvable discr={discr:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}"
                    )
                });
            if let mir::TerminatorKind::SwitchInt { targets, .. } = &term_ref.kind {
                let value_arms: Vec<(u128, mir::BasicBlock)> = targets.iter().map(|(v, bb)| (v, bb)).collect();
                let otherwise_bb = targets.otherwise();
                let discr_ty = discr.ty(local_decls, tcx);
                let is_bool = matches!(discr_ty.kind(), ty::TyKind::Bool);

                if is_bool && value_arms.len() == 1 {
                    let false_bb = mir_util::remap_bb_target(value_arms[0].1, mir_to_emitted)
                        .unwrap_or_else(|| panic!("lowering error: SwitchInt false target missing discr={discr:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}"));
                    let true_bb = mir_util::remap_bb_target(otherwise_bb, mir_to_emitted)
                        .unwrap_or_else(|| panic!("lowering error: SwitchInt true target missing discr={discr:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}"));
                    return Terminator::Branch { cond, true_bb, false_bb };
                }

                let mut switch_targets: Vec<(String, u32)> = Vec::with_capacity(value_arms.len());
                for (val, bb) in value_arms.iter() {
                    let remapped = mir_util::remap_bb_target(*bb, mir_to_emitted)
                        .unwrap_or_else(|| panic!("lowering error: SwitchInt target missing discr={discr:?} target={bb:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}"));
                    switch_targets.push((val.to_string(), remapped));
                }
                let switch_otherwise = mir_util::remap_bb_target(otherwise_bb, mir_to_emitted)
                    .unwrap_or_else(|| panic!("lowering error: SwitchInt otherwise target missing discr={discr:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}"));

                return Terminator::Switch { discr: cond, targets: switch_targets, otherwise: Some(switch_otherwise) };
            }
            panic!("lowering error: SwitchInt missing targets discr={discr:?} mir_variant=SwitchInt lowering_stage=terminator.lower_non_call file={term_file} span={term_span_str}");
        }
        mir::TerminatorKind::UnwindResume => {
            panic!("lowering error: unexpected UnwindResume in non-cleanup lowering");
        }
        mir::TerminatorKind::UnwindTerminate(_) => {
            panic!("lowering error: unexpected UnwindTerminate in non-cleanup lowering");
        }
        mir::TerminatorKind::Unreachable => Terminator::Unreachable,
        mir::TerminatorKind::Call { .. } => {
            panic!("lowering error: Call terminator reached in non-call lowering");
        }
        mir::TerminatorKind::TailCall { func, args, .. } => {
            let dest = "__ret".to_string();
            if let Some(method_stmt) = mir_ops::mir_method_call_stmt(tcx, local_decls, func, args, resolver, dest.clone()) {
                if let Stmt::MethodCall { dest: Some(dest), .. } = &method_stmt {
                    defined.insert(dest.clone());
                }
                stmts.push(method_stmt);
            } else if let Some(call_stmt) = mir_ops::mir_call_stmt(tcx, local_decls, func, args, resolver, dest.clone()) {
                if let Stmt::Call { dest: Some(dest), .. } = &call_stmt {
                    defined.insert(dest.clone());
                }
                stmts.push(call_stmt);
            } else if let Some(func_label) = mir_ops::mir_operand_label(tcx, func, resolver) {
                let arg_exprs: Vec<String> = args
                    .iter()
                    .map(|a| mir_ops::mir_operand_label(tcx, &a.node, resolver))
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_else(|| {
                        panic!("lowering error: TailCall args unresolvable func={func:?} args={args:?}");
                    });
                stmts.push(Stmt::Assign { lhs: dest.clone(), rhs: format!("{}({})", func_label, arg_exprs.join(", ")) });
                defined.insert(dest);
            } else {
                panic!("lowering error: TailCall func unresolvable func={func:?}");
            }
            Terminator::None
        }
        mir::TerminatorKind::Yield { .. } => {
            panic!("lowering error: Yield terminator unsupported in capture");
        }
        mir::TerminatorKind::CoroutineDrop => {
            panic!("lowering error: CoroutineDrop terminator unsupported in capture");
        }
        mir::TerminatorKind::FalseEdge { real_target, .. } => remap_to_goto(*real_target, mir_to_emitted),
        mir::TerminatorKind::FalseUnwind { real_target, .. } => remap_to_goto(*real_target, mir_to_emitted),
        mir::TerminatorKind::InlineAsm { targets, operands, .. } => {
            for op in operands.iter() {
                let out_place = match op {
                    mir::InlineAsmOperand::Out { place: Some(place), .. } => Some(place),
                    mir::InlineAsmOperand::InOut { out_place: Some(place), .. } => Some(place),
                    _ => None,
                };
                if let Some(place) = out_place {
                    let Some(dest) = mir_util::label_place_dest(resolver, place) else {
                        panic!("lowering error: InlineAsm output place unresolvable place={place:?}");
                    };
                    let ty = local_decls[place.local].ty;
                    let ty_str = crate::capture::pipeline::helpers::render_type_expr(
                        tcx,
                        &crate::capture::pipeline::helpers::lower_ty(tcx, ty),
                    );
                    stmts.push(Stmt::Assign {
                        lhs: dest.clone(),
                        rhs: format!("unsafe {{ std::mem::MaybeUninit::<{ty_str}>::uninit().assume_init() }}"),
                    });
                    defined.insert(dest);
                }
            }
            let mut target_iter = targets.iter();
            if let Some(target) = target_iter.next() {
                let remapped = mir_util::remap_bb_target(*target, mir_to_emitted)
                    .unwrap_or_else(|| panic!("lowering error: InlineAsm target missing target={target:?}"));
                return Terminator::Goto(remapped);
            }
            Terminator::Unreachable
        }
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

fn lower_return_terminator(returns_unit: bool, stmts: &mut Vec<Stmt>, _has_ret_binding: bool, _has_match_dest: bool) {
    if returns_unit {
        stmts.push(Stmt::Return(None));
    } else {
        // Emit structural return. __ret may not yet be in `defined` at this
        // point because finalization (stage_finalize_body_with_ret_fallback)
        // runs after all blocks are emitted and synthesizes the binding if
        // missing. Checking `defined` here is premature — that stage is the
        // authoritative gate for return integrity.
        stmts.push(Stmt::Return(Some("__ret".to_string())));
    }
}
