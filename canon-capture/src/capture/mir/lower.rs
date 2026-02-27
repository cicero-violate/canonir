use crate::types::{Body, Stmt};
use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;

use crate::norm;

use super::resolver::LocalNameResolver;

pub(crate) fn mir_body_structural(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    param_names: &[String],
    returns_unit: bool,
) -> Body {
    crate::project::item::mir_body_structural_legacy(tcx, def_id, param_names, returns_unit)
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

pub(crate) fn strip_instance_generics(raw: &str) -> String {
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

pub(crate) fn is_zero_arg_enum_ctor_expr_str(expr: &str) -> bool {
    let expr = strip_instance_generics(expr);
    expr == "std::option::Option::None"
        || expr == "core::option::Option::None"
        || expr == "Option::None"
}

pub(crate) fn is_zero_arg_enum_ctor_use(tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
    let mir::Rvalue::Use(mir::Operand::Constant(c)) = rvalue else {
        return false;
    };
    if let ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
        if matches!(
            tcx.def_kind(*did),
            rustc_hir::def::DefKind::Ctor(rustc_hir::def::CtorOf::Variant, rustc_hir::def::CtorKind::Const)
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

pub(crate) fn is_internal_mir_const_repr(s: &str) -> bool {
    s.contains("{alloc")
        || s.starts_with("alloc")
        || s.contains("promoted[")
}

pub(crate) fn is_filtered_internal_call_path(path: &str) -> bool {
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

pub(crate) fn path_has_unresolved_generic(path: &str) -> bool {
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
        ty::TyKind::FnDef(..)
            | ty::TyKind::Closure(..)
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
        mir::Operand::Copy(place) | mir::Operand::Move(place) => resolver.label_place(place),
        mir::Operand::Constant(c) => {
            if let ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
                return Some(norm::path(tcx, *did));
            }
            if let mir::Const::Unevaluated(uneval, _) = c.const_ {
                Some(norm::path(tcx, uneval.def))
            } else {
                let const_str = c.const_.to_string();
                if const_str.is_empty() || const_str == "_" || is_internal_mir_const_repr(&const_str) {
                    None
                } else {
                    Some(strip_instance_generics(&const_str))
                }
            }
        }
        mir::Operand::RuntimeChecks(_) => None,
    }
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
            ArgLabel::Omit => {}
        }
    }
    Some(out)
}

pub(crate) fn filtered_internal_call_target<'tcx>(
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

pub(crate) fn mir_method_call_stmt<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: &mir::Operand<'tcx>,
    args: &[rustc_span::source_map::Spanned<mir::Operand<'tcx>>],
    destination: &mir::Place<'tcx>,
    resolver: &LocalNameResolver,
) -> Option<Stmt> {
    let (did, _) = func.const_fn_def()?;
    if !matches!(tcx.def_kind(did), rustc_hir::def::DefKind::AssocFn) || args.is_empty() {
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

pub(crate) fn mir_call_stmt<'tcx>(
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
