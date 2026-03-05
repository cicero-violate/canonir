use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};

use crate::capture::helpers::{lower_ty, render_type_expr};
use crate::capture::mir::filters;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::norm;
use crate::types::Stmt;

pub(crate) enum ArgLabel {
    Value(String),
    Omit,
}

pub(crate) fn mir_operand_label_for_arg<'tcx>(tcx: TyCtxt<'tcx>, operand: &mir::Operand<'tcx>, resolver: &LocalNameResolver, local_decls: Option<&mir::LocalDecls<'tcx>>) -> Option<ArgLabel> {
    if let mir::Operand::Constant(c) = operand {
        if let Some(expr) = closure_placeholder_expr_from_const(tcx, c) {
            return Some(ArgLabel::Value(expr));
        }
    }
    match operand {
        mir::Operand::Constant(c) if constant_is_implicit_zst_value(c) => Some(ArgLabel::Omit),
        _ => mir_operand_label_with_decls(tcx, operand, resolver, local_decls).map(ArgLabel::Value),
    }
}

fn constant_is_implicit_zst_value(constant: &mir::ConstOperand<'_>) -> bool {
    matches!(constant.const_.ty().kind(), ty::TyKind::Closure(..) | ty::TyKind::Coroutine(..) | ty::TyKind::CoroutineClosure(..))
}

pub(crate) fn mir_operand_label<'tcx>(tcx: TyCtxt<'tcx>, operand: &mir::Operand<'tcx>, resolver: &LocalNameResolver) -> Option<String> {
    mir_operand_label_with_decls(tcx, operand, resolver, None)
}

pub(crate) fn mir_operand_label_with_decls<'tcx>(tcx: TyCtxt<'tcx>, operand: &mir::Operand<'tcx>, resolver: &LocalNameResolver, local_decls: Option<&mir::LocalDecls<'tcx>>) -> Option<String> {
    match operand {
        mir::Operand::Copy(place) | mir::Operand::Move(place) => {
            if let Some(label) = label_operand_place(place, resolver) {
                return Some(label);
            }
            if let Some(decls) = local_decls {
                let ty = decls[place.local].ty;
                if is_closure_or_fn_ptr(ty) {
                    return resolver.label_local(place.local);
                }
            }
            None
        }
        mir::Operand::Constant(c) => {
            if let Some(lit) = const_literal_expr(tcx, c) {
                return Some(lit);
            }
            if let Some(expr) = closure_placeholder_expr_from_const(tcx, c) {
                return Some(expr);
            }
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
    tcx: TyCtxt<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], resolver: &LocalNameResolver, local_decls: &mir::LocalDecls<'tcx>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match mir_operand_label_for_arg(tcx, &arg.node, resolver, Some(local_decls))? {
            ArgLabel::Value(v) => out.push(v),
            ArgLabel::Omit => return None,
        }
    }
    Some(out)
}

pub(crate) fn filtered_internal_call_target<'tcx>(tcx: TyCtxt<'tcx>, func: &mir::Operand<'tcx>, resolver: &LocalNameResolver) -> bool {
    if let Some(path) = call_target_path(tcx, func)
        && filters::is_filtered_internal_call_path(&path)
    {
        return true;
    }
    mir_operand_label(tcx, func, resolver).map(|path| filters::is_filtered_internal_call_path(&path)).unwrap_or(false)
}

pub(crate) fn call_target_path<'tcx>(tcx: TyCtxt<'tcx>, func: &mir::Operand<'tcx>) -> Option<String> {
    let (did, _) = func.const_fn_def()?;
    Some(norm::path(tcx, did))
}

pub(crate) fn is_format_call_target<'tcx>(tcx: TyCtxt<'tcx>, func: &mir::Operand<'tcx>) -> bool {
    let Some(path) = call_target_path(tcx, func) else {
        return false;
    };
    matches!(path.as_str(), "std::fmt::format" | "core::fmt::format" | "alloc::fmt::format") || (path.contains("fmt::") && path.ends_with("::format"))
}

pub(crate) fn is_deref_call_target<'tcx>(tcx: TyCtxt<'tcx>, func: &mir::Operand<'tcx>, resolver: &LocalNameResolver) -> bool {
    let by_path = call_target_path(tcx, func).map(|p| p.contains("ops::deref::Deref::deref")).unwrap_or(false);
    if by_path {
        return true;
    }
    mir_operand_label(tcx, func, resolver).map(|f| f.ends_with("::deref")).unwrap_or(false)
}

pub(crate) fn mir_method_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, func: &mir::Operand<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], resolver: &LocalNameResolver, dest: String,
) -> Option<Stmt> {
    if args.is_empty() {
        return None;
    }
    if let Some((did, _)) = func.const_fn_def() {
        if !matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn) {
            return None;
        }
        let assoc = tcx.associated_item(did);
        let has_self_param = matches!(assoc.kind, ty::AssocKind::Fn { has_self: true, .. });
        if !has_self_param {
            return None;
        }
        let receiver = match mir_operand_label_for_arg(tcx, &args[0].node, resolver, Some(local_decls))? {
            ArgLabel::Value(v) => v,
            ArgLabel::Omit => return None,
        };
        let method = tcx.item_name(did).to_string();
        let args = mir_call_args_labels(tcx, &args[1..], resolver, local_decls)?;
        return Some(Stmt::MethodCall { receiver, method, args, dest: Some(dest) });
    }

    let func_label = mir_operand_label(tcx, func, resolver)?;
    let method = dynamic_trait_method_name(&func_label)?;
    let receiver = match mir_operand_label_for_arg(tcx, &args[0].node, resolver, Some(local_decls))? {
        ArgLabel::Value(v) => v,
        ArgLabel::Omit => return None,
    };
    let args = mir_call_args_labels(tcx, &args[1..], resolver, local_decls)?;
    Some(Stmt::MethodCall { receiver, method, args, dest: Some(dest) })
}

pub(crate) fn mir_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, func: &mir::Operand<'tcx>, args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>], resolver: &LocalNameResolver, dest: String,
) -> Option<Stmt> {
    let func = if let Some((did, _)) = func.const_fn_def() {
        if matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn) {
            let assoc = tcx.associated_item(did);
            if matches!(assoc.container, ty::AssocContainer::InherentImpl | ty::AssocContainer::TraitImpl(_)) {
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
    let args = mir_call_args_labels(tcx, args, resolver, local_decls)?;
    Some(Stmt::Call { func, args, dest: Some(dest) })
}

fn dynamic_trait_method_name(func_label: &str) -> Option<String> {
    if !func_label.contains(" as ") {
        return None;
    }
    let method = func_label.rsplit("::").next()?;
    if method.is_empty() || !method.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(method.to_string())
}

fn is_closure_or_fn_ptr(ty: ty::Ty<'_>) -> bool {
    matches!(ty.kind(), ty::TyKind::Closure(..) | ty::TyKind::Coroutine(..) | ty::TyKind::CoroutineClosure(..) | ty::TyKind::FnPtr(..) | ty::TyKind::FnDef(..))
}

fn closure_placeholder_expr_from_const<'tcx>(tcx: TyCtxt<'tcx>, constant: &mir::ConstOperand<'tcx>) -> Option<String> {
    match constant.const_.ty().kind() {
        ty::TyKind::Closure(..) | ty::TyKind::Coroutine(..) | ty::TyKind::CoroutineClosure(..) => {
            closure_placeholder_expr_from_ty(tcx, constant.const_.ty())
        }
        _ => None,
    }
}

fn const_literal_expr<'tcx>(tcx: TyCtxt<'tcx>, constant: &mir::ConstOperand<'tcx>) -> Option<String> {
    let (inner_ty, ref_depth) = peel_ref_layers(constant.const_.ty());
    let mut lit = const_span_literal(tcx, constant)
        .or_else(|| unevaluated_anon_const_literal(tcx, constant))?;

    let builtin_refs = builtin_ref_layers(&lit, inner_ty);
    if ref_depth > builtin_refs {
        let extra = "&".repeat(ref_depth - builtin_refs);
        lit = format!("{extra}{lit}");
    }
    Some(lit)
}

fn peel_ref_layers<'tcx>(mut ty: ty::Ty<'tcx>) -> (ty::Ty<'tcx>, usize) {
    let mut depth = 0usize;
    while let ty::TyKind::Ref(_, inner, _) = ty.kind() {
        depth += 1;
        ty = *inner;
    }
    (ty, depth)
}

fn const_span_literal(tcx: TyCtxt<'_>, constant: &mir::ConstOperand<'_>) -> Option<String> {
    let snippet = tcx.sess.source_map().span_to_snippet(constant.span).ok()?;
    let trimmed = snippet.trim();
    if is_string_like_literal(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

fn unevaluated_anon_const_literal<'tcx>(tcx: TyCtxt<'tcx>, constant: &mir::ConstOperand<'tcx>) -> Option<String> {
    let mir::Const::Unevaluated(uneval, _) = constant.const_ else {
        return None;
    };
    let local = uneval.def.as_local()?;
    let rustc_hir::Node::AnonConst(anon) = tcx.hir_node_by_def_id(local) else {
        return None;
    };
    let body = tcx.hir_body(anon.body);
    let snippet = tcx.sess.source_map().span_to_snippet(body.value.span).ok()?;
    let trimmed = snippet.trim();
    if is_string_like_literal(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn is_string_like_literal(s: &str) -> bool {
    let t = s.trim_start_matches('&').trim();
    t.starts_with('"') || t.starts_with("r\"") || t.starts_with("b\"") || t.starts_with("br\"")
}

fn builtin_ref_layers(inner_lit: &str, inner_ty: ty::Ty<'_>) -> usize {
    let trimmed = inner_lit.trim();
    if trimmed.starts_with('&') {
        return 1;
    }
    match inner_ty.kind() {
        ty::TyKind::Str => 1,
        ty::TyKind::Slice(elem) if matches!(elem.kind(), ty::TyKind::Uint(ty::UintTy::U8)) => 1,
        _ => 0,
    }
}

pub(crate) fn closure_placeholder_expr_from_aggregate<'tcx>(
    tcx: TyCtxt<'tcx>,
    _def_id: rustc_span::def_id::DefId,
    args: ty::GenericArgsRef<'tcx>,
) -> Option<String> {
    let closure_args = args.as_closure();
    closure_placeholder_expr_from_sig(tcx, closure_args.sig())
}

fn closure_placeholder_expr_from_ty<'tcx>(tcx: TyCtxt<'tcx>, ty: ty::Ty<'tcx>) -> Option<String> {
    match ty.kind() {
        ty::TyKind::Closure(_def_id, args) => {
            let closure_args = args.as_closure();
            closure_placeholder_expr_from_sig(tcx, closure_args.sig())
        }
        _ => None,
    }
}

fn closure_placeholder_expr_from_sig<'tcx>(tcx: TyCtxt<'tcx>, sig: ty::Binder<'tcx, ty::FnSig<'tcx>>) -> Option<String> {
    let inputs = sig.inputs().skip_binder();
    let output = sig.output().skip_binder();

    let mut arg_tys: Vec<ty::Ty<'tcx>> = inputs.iter().copied().collect();
    // Closure signatures use the rust-call ABI where arguments are often
    // packed into a single tuple. Emit canonical fn(T1, T2, ..) placeholders.
    if arg_tys.len() == 1 {
        if let ty::TyKind::Tuple(items) = arg_tys[0].kind() {
            arg_tys = items.iter().copied().collect();
        }
    }

    let param_types = arg_tys
        .iter()
        .map(|ty| normalize_singleton_tuple_type(render_type_expr(tcx, &lower_ty(tcx, *ty))))
        .collect::<Vec<_>>()
        .join(", ");

    let args = arg_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let name = format!("_arg{i}");
            let ty_expr = normalize_singleton_tuple_type(render_type_expr(tcx, &lower_ty(tcx, *ty)));
            format!("{name}: {ty_expr}")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let ret_expr = render_type_expr(tcx, &lower_ty(tcx, output));
    let closure = if args.is_empty() {
        format!("|| -> {ret_expr} {{ panic!(\"canon closure placeholder\") }}")
    } else {
        format!("|{args}| -> {ret_expr} {{ panic!(\"canon closure placeholder\") }}")
    };
    Some(format!("({closure}) as fn({param_types}) -> {ret_expr}"))
}

fn normalize_singleton_tuple_type(rendered: String) -> String {
    let s = rendered.trim();
    let Some(inner) = s.strip_prefix('(').and_then(|v| v.strip_suffix(')')) else {
        return rendered;
    };

    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut comma_positions: Vec<usize> = Vec::new();
    for (idx, ch) in inner.char_indices() {
        match ch {
            '<' => angle += 1,
            '>' => angle -= 1,
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            ',' if angle == 0 && paren == 0 && bracket == 0 => comma_positions.push(idx),
            _ => {}
        }
    }
    if comma_positions.len() != 1 {
        return rendered;
    }
    let comma = comma_positions[0];
    if !inner[comma + 1..].trim().is_empty() {
        return rendered;
    }
    let elem = inner[..comma].trim();
    if elem.is_empty() {
        return rendered;
    }
    elem.to_string()
}
