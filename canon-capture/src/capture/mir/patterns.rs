use rustc_middle::mir;
use rustc_middle::ty::TyCtxt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirOpKind {
    FieldAccess,
    StructLit,
    OpaqueAggregate,
    ZeroArgEnumCtor,
    ConstUse,
    Assign,
}

pub struct MirPattern {
    pub kind: MirOpKind,
    pub predicate: for<'tcx> fn(TyCtxt<'tcx>, &mir::Rvalue<'tcx>) -> bool,
}

fn is_field_access_pattern(_tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
    matches!(
        rvalue,
        mir::Rvalue::Use(mir::Operand::Copy(place) | mir::Operand::Move(place))
            if matches!(place.as_ref().last_projection(), Some((_, mir::ProjectionElem::Field(..))))
    )
}

fn is_struct_lit_pattern(_tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
    matches!(rvalue, mir::Rvalue::Aggregate(kind, _) if matches!(&**kind, mir::AggregateKind::Adt(_, _, _, _, _)))
}

fn is_opaque_aggregate_pattern(_tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
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

fn is_zero_arg_enum_ctor_pattern(tcx: TyCtxt<'_>, rvalue: &mir::Rvalue<'_>) -> bool {
    let mir::Rvalue::Use(mir::Operand::Constant(c)) = rvalue else {
        return false;
    };
    if let rustc_middle::ty::TyKind::FnDef(did, _) = c.const_.ty().kind() {
        if matches!(
            tcx.def_kind(*did),
            rustc_hir::def::DefKind::Ctor(
                rustc_hir::def::CtorOf::Variant,
                rustc_hir::def::CtorKind::Const
            )
        ) {
            return true;
        }
    }
    let rustc_middle::ty::TyKind::Adt(adt, _) = c.const_.ty().kind() else {
        return false;
    };
    if !adt.is_enum() {
        return false;
    }
    if let mir::Const::Val(v, ty) = c.const_ {
        if v.try_to_scalar_int().is_some()
            && matches!(ty.kind(), rustc_middle::ty::TyKind::Adt(adt2, _) if adt2.is_enum())
            && adt.variants().iter().any(|var| var.fields.is_empty())
        {
            return true;
        }
    }
    false
}

static MIR_PATTERNS: &[MirPattern] = &[
    MirPattern {
        kind: MirOpKind::FieldAccess,
        predicate: is_field_access_pattern,
    },
    MirPattern {
        kind: MirOpKind::StructLit,
        predicate: is_struct_lit_pattern,
    },
    MirPattern {
        kind: MirOpKind::OpaqueAggregate,
        predicate: is_opaque_aggregate_pattern,
    },
    MirPattern {
        kind: MirOpKind::ZeroArgEnumCtor,
        predicate: is_zero_arg_enum_ctor_pattern,
    },
    MirPattern {
        kind: MirOpKind::ConstUse,
        predicate: |_tcx, rvalue| matches!(rvalue, mir::Rvalue::Use(mir::Operand::Constant(_))),
    },
];

pub fn dispatch_stmt_pattern<'tcx>(tcx: TyCtxt<'tcx>, rvalue: &mir::Rvalue<'tcx>) -> MirOpKind {
    for pattern in MIR_PATTERNS {
        if (pattern.predicate)(tcx, rvalue) {
            return pattern.kind;
        }
    }
    MirOpKind::Assign
}
