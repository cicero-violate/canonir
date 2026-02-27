use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};

use crate::capture::helpers::{lower_ty, render_type_expr};
use crate::capture::mir::filters;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::types::Stmt;
use crate::norm;

pub(crate) enum ArgLabel {
    Value(String),
    Omit,
}

pub(crate) fn mir_operand_label_for_arg(
    tcx: TyCtxt<'_>,
    operand: &mir::Operand<'_>,
    resolver: &LocalNameResolver,
) -> Option<ArgLabel> {
    match operand {
        mir::Operand::Constant(c) if constant_is_implicit_zst_value(c) => Some(ArgLabel::Omit),
        _ => mir_operand_label(tcx, operand, resolver).map(ArgLabel::Value),
    }
}

fn constant_is_implicit_zst_value(constant: &mir::ConstOperand<'_>) -> bool {
    matches!(
        constant.const_.ty().kind(),
        ty::TyKind::Closure(..)
            | ty::TyKind::Coroutine(..)
            | ty::TyKind::CoroutineClosure(..)
    )
}

pub(crate) fn mir_operand_label(
    tcx: TyCtxt<'_>,
    operand: &mir::Operand<'_>,
    resolver: &LocalNameResolver,
) -> Option<String> {
    match operand {
        mir::Operand::Copy(place) | mir::Operand::Move(place) => label_operand_place(place, resolver),
        mir::Operand::Constant(c) => {
            if let ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
                return Some(norm::path(tcx, *did));
            }
            if let mir::Const::Unevaluated(uneval, _) = c.const_ {
                Some(norm::path(tcx, uneval.def))
            } else {
                let const_str = c.const_.to_string();
                if const_str.is_empty() || const_str == "_" || filters::is_internal_mir_const_repr(&const_str) {
                    None
                } else {
                    Some(filters::strip_instance_generics(&const_str))
                }
            }
        }
        mir::Operand::RuntimeChecks(_) => None,
    }
}

fn label_operand_place(place: &mir::Place<'_>, resolver: &LocalNameResolver) -> Option<String> {
    if place.projection.is_empty() {
        return resolver.label_place(place);
    }
    // Operand-side projection rendering is intentionally strict because this path
    // lacks full type context and can otherwise leak MIR-only field accesses.
    if place.projection.len() == 1 && matches!(place.projection[0], mir::ProjectionElem::Deref) {
        return Some(format!("*{}", resolver.label_local(place.local)?));
    }
    None
}

pub(crate) fn mir_call_args_labels<'tcx>(
    tcx: TyCtxt<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    resolver: &LocalNameResolver,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match mir_operand_label_for_arg(tcx, &arg.node, resolver)? {
            ArgLabel::Value(v) => out.push(v),
            ArgLabel::Omit => return None,
        }
    }
    Some(out)
}

pub(crate) fn filtered_internal_call_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    resolver: &LocalNameResolver,
) -> bool {
    if let Some(path) = call_target_path(tcx, func)
        && filters::is_filtered_internal_call_path(&path)
    {
        return true;
    }
    mir_operand_label(tcx, func, resolver)
        .map(|path| filters::is_filtered_internal_call_path(&path))
        .unwrap_or(false)
}

pub(crate) fn call_target_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
) -> Option<String> {
    let (did, _) = func.const_fn_def()?;
    Some(norm::path(tcx, did))
}

pub(crate) fn is_format_call_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
) -> bool {
    let Some(path) = call_target_path(tcx, func) else {
        return false;
    };
    matches!(path.as_str(), "std::fmt::format" | "core::fmt::format" | "alloc::fmt::format")
        || (path.contains("fmt::") && path.ends_with("::format"))
}

pub(crate) fn is_deref_call_target<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    resolver: &LocalNameResolver,
) -> bool {
    let by_path = call_target_path(tcx, func)
        .map(|p| p.contains("ops::deref::Deref::deref"))
        .unwrap_or(false);
    if by_path {
        return true;
    }
    mir_operand_label(tcx, func, resolver)
        .map(|f| f.ends_with("::deref"))
        .unwrap_or(false)
}

pub(crate) fn mir_method_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    resolver: &LocalNameResolver,
    dest: String,
) -> Option<Stmt> {
    let (did, _) = func.const_fn_def()?;
    if !matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn) || args.is_empty() {
        return None;
    }
    let assoc = tcx.associated_item(did);
    let has_self_param = matches!(assoc.kind, ty::AssocKind::Fn { has_self: true, .. });
    if !has_self_param {
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
        dest: Some(dest),
    })
}

pub(crate) fn mir_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    resolver: &LocalNameResolver,
    dest: String,
) -> Option<Stmt> {
    let func = if let Some((did, _)) = func.const_fn_def() {
        if matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn) {
            let assoc = tcx.associated_item(did);
            if matches!(
                assoc.container,
                ty::AssocContainer::InherentImpl | ty::AssocContainer::TraitImpl(_)
            ) {
                let impl_did = assoc.container_id(tcx);
                let self_ty = tcx.type_of(impl_did).instantiate_identity();
                let self_path = render_type_expr(tcx, &lower_ty(tcx, self_ty));
                let method = tcx.item_name(did).to_string();
                format!("{self_path}::{method}")
            } else {
                filters::strip_instance_generics(&norm::path(tcx, did))
            }
        } else {
            filters::strip_instance_generics(&norm::path(tcx, did))
        }
    } else {
        filters::strip_instance_generics(&mir_operand_label(tcx, func, resolver)?)
    };
    if filters::path_has_unresolved_generic(&func) {
        return None;
    }
    let args = mir_call_args_labels(tcx, args, resolver)?;
    Some(Stmt::Call {
        func,
        args,
        dest: Some(dest),
    })
}
