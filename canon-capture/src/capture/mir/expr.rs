use rustc_abi::{FieldIdx, VariantIdx};
use rustc_middle::mir;
use rustc_middle::ty::{self, TyCtxt};

use crate::capture::helpers::{lower_ty, render_type_expr};
use crate::capture::mir::filters;
use crate::capture::mir::guard as mir_guard;
use crate::capture::mir::ops as mir_ops;
use crate::capture::mir::resolver::LocalNameResolver;
use crate::norm;
use crate::types::{Stmt, TypeExpr};

pub(crate) fn render_projected_place_expr<'tcx>(tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, place: &mir::Place<'tcx>, resolver: &LocalNameResolver) -> Option<String> {
    if place.projection.is_empty() {
        return resolver.label_place(place);
    }
    let mut expr = resolver.label_local(place.local)?;
    let mut cursor_ty = local_decls[place.local].ty;
    let mut pending_downcast: Option<(VariantIdx, ty::Ty<'tcx>)> = None;
    for elem in place.projection.iter() {
        match elem {
            mir::ProjectionElem::Deref => {
                // Avoid eagerly materializing `*expr` in the rendered output.
                // Emitting explicit deref here interacts poorly with enum
                // downcast projection, producing `match *self` and moving out
                // of borrowed values. Instead, keep the base expression
                // unchanged and only advance the type cursor.
                cursor_ty = match cursor_ty.kind() {
                    ty::TyKind::Ref(_, inner, _) => *inner,
                    ty::TyKind::RawPtr(inner, _) => *inner,
                    _ => cursor_ty.builtin_deref(true)?,
                };
            }
            mir::ProjectionElem::Downcast(variant_name, variant_idx) => {
                let _ = variant_name;
                pending_downcast = Some((variant_idx, cursor_ty));
            }
            mir::ProjectionElem::Field(field_idx, field_ty) => {
                let field = if let Some(downcast) = pending_downcast.take() {
                    expr = render_downcast_field_expr(tcx, &expr, downcast.1, downcast.0, field_idx)?;
                    String::new()
                } else {
                    match cursor_ty.kind() {
                        ty::TyKind::Adt(adt, _) => {
                            if !adt.did().is_local() {
                                let tuple_like = adt.non_enum_variant().fields.iter().all(|f| f.name.to_string().chars().all(|c| c.is_ascii_digit()));
                                if tuple_like {
                                    return None;
                                }
                            }
                            let f = adt.non_enum_variant().fields.get(field_idx)?;
                            let name = f.name.to_string();
                            if name.chars().all(|c| c.is_ascii_digit()) {
                                field_idx.index().to_string()
                            } else {
                                name
                            }
                        }
                        ty::TyKind::Tuple(_) => field_idx.index().to_string(),
                        _ => return None,
                    }
                };
                if !field.is_empty() {
                    expr = format!("({expr}).{field}");
                }
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
    if let Some(downcast) = pending_downcast {
        let ty::TyKind::Adt(adt, _) = downcast.1.kind() else {
            return None;
        };
        if !adt.is_enum() {
            return None;
        }
        let enum_path = norm::path(tcx, adt.did());
        let variant = adt.variant(downcast.0);
        let variant_path = format!("{enum_path}::{}", variant.name);
        // Match on a reference to avoid moving out of borrowed values (e.g., &self).
        expr = format!(
            "match &{expr} {{ {variant_path} => (), _ => panic!(\"canon downcast projection mismatch\") }}"
        );
    }
    Some(expr)
}

fn render_downcast_field_expr<'tcx>(tcx: TyCtxt<'tcx>, base_expr: &str, enum_ty: ty::Ty<'tcx>, variant_idx: VariantIdx, field_idx: FieldIdx) -> Option<String> {
    let ty::TyKind::Adt(adt, _) = enum_ty.kind() else {
        return None;
    };
    if !adt.is_enum() {
        return None;
    }
    let variant = adt.variant(variant_idx);
    let enum_path = norm::path(tcx, adt.did());
    let variant_path = format!("{enum_path}::{}", variant.name);
    let idx = field_idx.index();
    if idx >= variant.fields.len() {
        return None;
    }

    let bindings: Vec<String> = (0..variant.fields.len()).map(|i| format!("__canon_f{i}")).collect();
    let select = bindings[idx].clone();
    let pattern = match &variant.fields {
        fields if fields.is_empty() => variant_path.clone(),
        fields
            if fields.iter().all(|f| {
                let name = f.name.to_string();
                !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit())
            }) =>
        {
            let named = fields.iter().zip(bindings.iter()).map(|(f, b)| format!("{}: {b}", f.name)).collect::<Vec<_>>().join(", ");
            format!("{variant_path} {{ {named} }}")
        }
        _ => {
            let tuple = bindings.join(", ");
            format!("{variant_path}({tuple})")
        }
    };

    // Match on a reference to avoid moving out of borrowed enum values (e.g., &self).
    // This prevents generated code like `match *self` from moving non-Copy fields.
    // Ensure we never accidentally match on a dereferenced value like `*self`.
    // If the base expression already starts with a deref, strip the leading `*`
    // and match on a reference to the underlying expression instead.
    let safe_base = base_expr.strip_prefix('*').unwrap_or(base_expr);
    Some(format!(
        "match &{safe_base} {{ {pattern} => {select}, _ => panic!(\"canon downcast projection mismatch\") }}"
    ))
}

pub(crate) fn mir_binop_token(op: mir::BinOp) -> Option<&'static str> {
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
        mir::BinOp::AddWithOverflow => None,
        mir::BinOp::SubWithOverflow => None,
        mir::BinOp::MulWithOverflow => None,
    }
}

pub(crate) fn mir_unop_token(op: mir::UnOp) -> &'static str {
    match op {
        mir::UnOp::Not => "!",
        mir::UnOp::Neg => "-",
        mir::UnOp::PtrMetadata => "",
    }
}

pub(crate) fn mir_assign_stmt<'tcx>(
    tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, lhs: &mir::Place<'tcx>, rvalue: &mir::Rvalue<'tcx>, resolver: &LocalNameResolver, defined: &std::collections::HashSet<String>,
    suppressed_sentinel_names: &std::collections::HashSet<String>,
) -> Option<Stmt> {
    let lhs = resolver.label_place(lhs)?;
    // If we cannot render the rvalue into a concrete expression, do NOT
    // silently collapse it to unit. That destroys type information and
    // propagates `()` into non-unit locals (Vec, Option, String, etc.),
    // leading to downstream type errors in emitted Rust.
    // Instead, return None so the caller can decide whether to
    // structurally suppress or handle the assignment.
    let rhs = match mir_rvalue_expr(tcx, local_decls, rvalue, resolver) {
        Some(expr) => expr,
        None => return None,
    };
    if assign_rhs_should_suppress(&rhs) {
        return None;
    }
    if lhs == "__ret" {
        // Do not allow panic-based synthetic initialization of __ret;
        // force canonical suppressed binding instead.
        if rhs.contains("panic!") {
            // Do not downgrade panic-based structural expressions to unit.
            // Preserve the diverging expression to maintain correct typing
            // and avoid propagating `()` into non-unit locals.
            return Some(Stmt::Assign { lhs, rhs });
        }
        return Some(Stmt::Assign { lhs, rhs });
    }
    if !mir_guard::value_known(&rhs, defined, suppressed_sentinel_names) {
        return None;
    }
    Some(Stmt::Assign { lhs, rhs })
}

pub(crate) fn mir_field_access_stmt<'tcx>(tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, lhs: &mir::Place<'tcx>, rvalue: &mir::Rvalue<'tcx>, resolver: &LocalNameResolver) -> Option<Stmt> {
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
            if !adt.did().is_local() {
                let tuple_like = adt.non_enum_variant().fields.iter().all(|f| f.name.to_string().chars().all(|c| c.is_ascii_digit()));
                if tuple_like {
                    return None;
                }
            }
            let f = adt.non_enum_variant().fields.get(field_idx)?;
            let name = f.name.to_string();
            if name.chars().all(|c| c.is_ascii_digit()) {
                field_idx.index().to_string()
            } else {
                name
            }
        }
        ty::TyKind::Adt(adt, _) if adt.is_enum() => {
            let downcast_idx = place.projection.iter().find_map(|elem| match elem {
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
    Some(Stmt::FieldAccess { base: resolver.label_place_ref(base)?, field, dest: Some(resolver.label_place(lhs)?) })
}

pub(crate) fn mir_struct_lit_stmt<'tcx>(tcx: TyCtxt<'tcx>, lhs: &mir::Place<'tcx>, rvalue: &mir::Rvalue<'tcx>, resolver: &LocalNameResolver) -> Option<Stmt> {
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
    let fields = variant.fields.iter().zip(operands.iter()).map(|(f, op)| Some((f.name.to_string(), mir_ops::mir_operand_label(tcx, op, resolver)?))).collect::<Option<Vec<_>>>()?;
    let ctor_path = if adt.is_enum() { format!("{}::{}", norm::path(tcx, *adt_did), variant.name) } else { norm::path(tcx, *adt_did) };
    Some(Stmt::StructLit { ty: TypeExpr::Path(ctor_path), fields, dest: Some(resolver.label_place(lhs)?) })
}

fn assign_rhs_should_suppress(rhs: &str) -> bool {
    filters::is_zero_arg_enum_ctor_expr_str(rhs) || rhs.contains("SizedTypeProperties") || rhs.contains("std::alloc::Global") || rhs.contains("alloc::Global")
}

fn mir_rvalue_expr<'tcx>(tcx: TyCtxt<'tcx>, local_decls: &mir::LocalDecls<'tcx>, rvalue: &mir::Rvalue<'tcx>, resolver: &LocalNameResolver) -> Option<String> {
    match rvalue {
        mir::Rvalue::Use(op) => match op {
            mir::Operand::Copy(place) | mir::Operand::Move(place) => {
                if place.projection.is_empty() {
                    resolver.label_place(place)
                } else {
                    render_projected_place_expr(tcx, local_decls, place, resolver)
                }
            }
            _ => mir_ops::mir_operand_label(tcx, op, resolver),
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
            Some(if matches!(raw_ptr_kind, mir::RawPtrKind::Mut) { format!("&mut {place}") } else { format!("&{place}") })
        }
        mir::Rvalue::BinaryOp(op, boxed) => {
            let (lhs, rhs) = &**boxed;
            let lhs = mir_ops::mir_operand_label(tcx, lhs, resolver)?;
            let rhs = mir_ops::mir_operand_label(tcx, rhs, resolver)?;
            match *op {
                mir::BinOp::AddWithOverflow => Some(format!("({lhs}).overflowing_add({rhs})")),
                mir::BinOp::SubWithOverflow => Some(format!("({lhs}).overflowing_sub({rhs})")),
                mir::BinOp::MulWithOverflow => Some(format!("({lhs}).overflowing_mul({rhs})")),
                _ => Some(format!("({lhs} {} {rhs})", mir_binop_token(*op)?)),
            }
        }
        mir::Rvalue::UnaryOp(op, operand) => Some(format!("({}{})", mir_unop_token(*op), mir_ops::mir_operand_label(tcx, operand, resolver)?)),
        mir::Rvalue::Cast(_, operand, ty) => Some(format!("({} as {})", mir_ops::mir_operand_label(tcx, operand, resolver)?, render_type_expr(tcx, &lower_ty(tcx, *ty)))),
        mir::Rvalue::Aggregate(kind, operands) => match &**kind {
            mir::AggregateKind::Adt(adt_did, variant_idx, _, _, _) => {
                let adt = tcx.adt_def(*adt_did);
                let variant = adt.variant(*variant_idx);
                if !variant.fields.is_empty() || !operands.is_empty() {
                    return None;
                }
                if adt.is_enum() {
                    Some(format!("{}::{}", norm::path(tcx, *adt_did), variant.name))
                } else {
                    Some(norm::path(tcx, *adt_did))
                }
            }
            mir::AggregateKind::Tuple => {
                let elems = operands.iter().map(|op| mir_ops::mir_operand_label(tcx, op, resolver)).collect::<Option<Vec<_>>>()?;
                if elems.len() == 1 {
                    Some(format!("({},)", elems[0]))
                } else {
                    Some(format!("({})", elems.join(", ")))
                }
            }
            mir::AggregateKind::Array(_) => {
                let elems = operands.iter().map(|op| mir_ops::mir_operand_label(tcx, op, resolver)).collect::<Option<Vec<_>>>()?;
                Some(format!("[{}]", elems.join(", ")))
            }
            _ => None,
        },
        mir::Rvalue::Repeat(operand, count) => {
            let count = count.try_to_target_usize(tcx)?;
            Some(format!("[{}; {count}]", mir_ops::mir_operand_label(tcx, operand, resolver)?))
        }
        mir::Rvalue::Discriminant(place) => Some(format!("{} as isize", resolver.label_place(place)?)),
        mir::Rvalue::CopyForDeref(place) => resolver.label_place(place),
        _ => None,
    }
}

fn is_primitive_value_ty(ty: ty::Ty<'_>) -> bool {
    matches!(ty.kind(), ty::TyKind::Bool | ty::TyKind::Char | ty::TyKind::Int(..) | ty::TyKind::Uint(..) | ty::TyKind::Float(..) | ty::TyKind::Str | ty::TyKind::Never)
}
