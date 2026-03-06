use crate::types::TraitMethod;
use crate::types::{Body, Field, GenericParam, Param, PrimType, TypeExpr, Visibility};
use rustc_hir::{def::DefKind, GenericBound, PatKind, PredicateOrigin, Safety, WherePredicateKind};
use rustc_middle::ty::AssocKind;
use rustc_middle::ty::{self, CoroutineArgsExt, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::hygiene::{ExpnKind, MacroKind};

use crate::norm;

pub(crate) fn map_vis(tcx: TyCtxt<'_>, def_id: DefId, v: ty::Visibility<DefId>) -> Visibility {
    match v {
        ty::Visibility::Public => Visibility::Public,
        ty::Visibility::Restricted(restricted) => {
            let crate_name = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).to_string();
            let restricted_path = tcx.def_path_str(restricted);
            if restricted_path == crate_name {
                return Visibility::PubCrate;
            }
            if let Some(local_def_id) = def_id.as_local() {
                let parent = tcx.parent_module_from_def_id(local_def_id);
                if restricted == parent.to_def_id() {
                    return Visibility::Private; // pub(self)
                }
            }
            let canonical = norm::path(tcx, restricted);
            if canonical.trim().is_empty() {
                // pub(in <path>) must carry a canonical non-empty path in ModelIR.
                // When rustc visibility resolves to an empty path, degrade to private
                // rather than emitting invalid PubIn payload.
                Visibility::Private
            } else if canonical == "crate" {
                Visibility::PubCrate
            } else {
                Visibility::PubIn(canonical)
            }
        }
    }
}

/// Returns `(generic_params, where_clause_strings)`.
///
/// Inline bounds: read from HIR `GenericParam.bounds` (written as `<T: Bound>`).
/// Where clauses: read from HIR `Generics.predicates` with `origin == WhereClause`.
/// Both are source snippets — exactly what the user wrote, no path normalization.
pub(crate) fn map_generics(tcx: TyCtxt<'_>, def_id: DefId) -> (Vec<GenericParam>, Vec<String>) {
    let supported = matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias);
    if !supported {
        return (Vec::new(), Vec::new());
    }

    let Some(local) = def_id.as_local() else {
        return map_generics_ty_fallback(tcx, def_id);
    };

    let hir_generics = match tcx.hir_node_by_def_id(local) {
        rustc_hir::Node::Item(item) => item.kind.generics(),
        rustc_hir::Node::TraitItem(ti) => Some(ti.generics),
        rustc_hir::Node::ImplItem(ii) => Some(ii.generics),
        _ => None,
    };
    let Some(hgen) = hir_generics else {
        return map_generics_ty_fallback(tcx, def_id);
    };

    let sm = tcx.sess.source_map();

    // Inline bounds: from GenericParam.bounds, skip Sized.
    let params: Vec<GenericParam> = hgen
        .params
        .iter()
        .filter(|p| {
            let n = p.name.ident().to_string();
            n != "Self" && !p.is_elided_lifetime() && !p.is_impl_trait()
        })
        .map(|p| {
            let name = p.name.ident().to_string();
            let is_lifetime = matches!(p.kind, rustc_hir::GenericParamKind::Lifetime { .. });
            // All bounds (inline and where) live in hgen.predicates since PR #93803.
            // Inline bounds have origin == GenericParam; where-clause bounds have origin == WhereClause.
            let bounds: Vec<String> = hgen
                .predicates
                .iter()
                .filter_map(|pred| {
                    if let WherePredicateKind::BoundPredicate(bp) = pred.kind {
                        if bp.origin == PredicateOrigin::GenericParam {
                            // Check that this predicate applies to our param by matching name.
                            let bounded_name = sm.span_to_snippet(bp.bounded_ty.span).unwrap_or_default();
                            if bounded_name == name {
                                return Some(bp.bounds);
                            }
                        }
                    }
                    None
                })
                .flat_map(|bounds| bounds.iter())
                .filter(|b| !is_sized_bound(b))
                .filter_map(|b| sm.span_to_snippet(b.span()).ok())
                .collect();
            GenericParam { name, bounds, is_lifetime, default_ty: None }
        })
        .collect();

    // Where clauses: predicates with origin == WhereClause.
    let mut where_clauses: Vec<String> = Vec::new();
    for pred in hgen.predicates {
        if !pred.kind.in_where_clause() {
            continue;
        }
        match pred.kind {
            WherePredicateKind::BoundPredicate(bp) => {
                let lhs = sm.span_to_snippet(bp.bounded_ty.span).unwrap_or_default();
                let rhs: Vec<String> = bp.bounds.iter().filter_map(|b| sm.span_to_snippet(b.span()).ok()).collect();
                if !lhs.is_empty() && !rhs.is_empty() {
                    where_clauses.push(format!("{}: {}", lhs, rhs.join(" + ")));
                }
            }
            WherePredicateKind::RegionPredicate(rp) => {
                let lhs = rp.lifetime.ident.to_string();
                let rhs: Vec<String> = rp.bounds.iter().filter_map(|b| sm.span_to_snippet(b.span()).ok()).collect();
                if !rhs.is_empty() {
                    where_clauses.push(format!("{}: {}", lhs, rhs.join(" + ")));
                }
            }
            _ => {}
        }
    }

    (params, where_clauses)
}

/// True if a HIR GenericBound is a `Sized` or `MetaSized` bound (implicit, skip it).
#[allow(dead_code)]
fn is_sized_bound(b: &GenericBound<'_>) -> bool {
    if let GenericBound::Trait(tr) = b {
        if let Some(seg) = tr.trait_ref.path.segments.last() {
            let n = seg.ident.name.as_str();
            return n == "Sized" || n == "MetaSized";
        }
    }
    false
}

/// Fallback for non-local DefIds: ty-query only, no where-clause split.
fn map_generics_ty_fallback(tcx: TyCtxt<'_>, def_id: DefId) -> (Vec<GenericParam>, Vec<String>) {
    let gens = tcx.generics_of(def_id);
    let params = gens
        .own_params
        .iter()
        .filter(|p| p.name.as_str() != "Self" && !p.name.as_str().starts_with("impl "))
        .map(|p| {
            let name = p.name.to_string();
            let is_lifetime = matches!(p.kind, ty::GenericParamDefKind::Lifetime);
            GenericParam { name, bounds: Vec::new(), is_lifetime, default_ty: None }
        })
        .collect();
    (params, Vec::new())
}

pub(crate) fn map_params<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId, inputs: &[ty::Ty<'tcx>]) -> Vec<Param> {
    let declared_tys = declared_fn_param_types(tcx, def_id).unwrap_or_default();
    let hir_names: Vec<Option<String>> = def_id
        .as_local()
        .and_then(|local| tcx.hir_maybe_body_owned_by(local))
        .map(|body| {
            body.params
                .iter()
                .map(|p| match p.pat.kind {
                    PatKind::Binding(_, _, ident, _) => Some(ident.to_string()),
                    PatKind::Wild => Some("_".to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    inputs
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let name = hir_names.get(i).and_then(|n| n.clone()).unwrap_or_else(|| format!("p{i}"));
            let is_self = name == "self";
            let ty_expr = if is_self {
                if matches!(ty.kind(), ty::TyKind::Ref(_, _, _)) {
                    TypeExpr::Ref { lifetime: None, inner: Box::new(TypeExpr::Param("Self".to_string())), mutable: false }
                } else {
                    TypeExpr::Param("Self".to_string())
                }
            } else {
                declared_tys.get(i).cloned().unwrap_or_else(|| lower_ty(tcx, *ty))
            };
            Param { name, ty: ty_expr, is_self, mutable: false, lifetime: None }
        })
        .collect()
}

pub(crate) fn map_fields<'a, I>(tcx: TyCtxt<'a>, fields: I, in_enum: bool) -> Vec<Field>
where I: Iterator<Item = &'a ty::FieldDef> {
    fields
        .map(|f| {
            let vis = if in_enum { Visibility::Private } else { map_vis(tcx, f.did, tcx.visibility(f.did)) };
            let raw_name = f.name.to_string();
            let name = if raw_name.chars().all(|c| c.is_ascii_digit()) { None } else { Some(raw_name) };
            let ty = declared_item_ty_expr(tcx, f.did).unwrap_or_else(|| lower_ty(tcx, tcx.type_of(f.did).instantiate_identity()));
            Field { name, ty, vis }
        })
        .collect()
}

/// For trait method declarations, HIR param names come from the TraitItem fn decl.
fn map_trait_method_params<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId, inputs: &[ty::Ty<'tcx>]) -> Vec<Param> {
    let declared_tys = declared_fn_param_types(tcx, def_id).unwrap_or_default();
    let hir_names: Vec<Option<String>> = def_id
        .as_local()
        .and_then(|local| {
            if let rustc_hir::Node::TraitItem(ti) = tcx.hir_node_by_def_id(local) {
                if let rustc_hir::TraitItemKind::Fn(fn_sig, _) = &ti.kind {
                    return Some(fn_sig.decl.inputs.iter().map(|_| None::<String>).collect());
                }
            }
            None
        })
        .unwrap_or_default();
    let has_self = inputs.first().map_or(false, |ty| {
        let s = ty.to_string();
        s == "Self" || s.ends_with("Self")
    });
    inputs
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let is_self = i == 0 && has_self;
            let name = if is_self { "self".to_string() } else { hir_names.get(i).and_then(|n| n.clone()).unwrap_or_else(|| format!("p{i}")) };
            let ty_expr = if is_self {
                if matches!(ty.kind(), ty::TyKind::Ref(_, _, _)) {
                    TypeExpr::Ref { lifetime: None, inner: Box::new(TypeExpr::Param("Self".to_string())), mutable: false }
                } else {
                    TypeExpr::Param("Self".to_string())
                }
            } else {
                declared_tys.get(i).cloned().unwrap_or_else(|| lower_ty(tcx, *ty))
            };
            Param { name, ty: ty_expr, is_self, mutable: false, lifetime: None }
        })
        .collect()
}

/// Slice the source text of a Const or Static initializer expression.
pub(crate) fn hir_init_src(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let Some(local) = def_id.as_local() else { return String::new() };
    let Some(body) = tcx.hir_maybe_body_owned_by(local) else { return String::new() };
    let span = body.value.span;
    let sm = tcx.sess.source_map();
    sm.span_to_snippet(span).unwrap_or_default()
}

/// Collect TraitMethod entries for a trait DefId from associated_items.
pub(crate) fn collect_trait_methods(tcx: TyCtxt<'_>, trait_def_id: DefId) -> Vec<TraitMethod> {
    tcx.associated_items(trait_def_id)
        .in_definition_order()
        .filter(|item| matches!(item.kind, AssocKind::Fn { .. }))
        .map(|item| {
            let sig = tcx.fn_sig(item.def_id).skip_binder();
            let params = map_trait_method_params(tcx, item.def_id, sig.inputs().skip_binder());
            let ret = declared_fn_return_type_expr(tcx, item.def_id).unwrap_or_else(|| lower_ty(tcx, sig.output().skip_binder()));
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(item.def_id).is_async();
            let (generics, _where_clauses) = map_generics(tcx, item.def_id);
            let vis = map_vis(tcx, item.def_id, tcx.visibility(item.def_id));
            TraitMethod { name: item.name().to_string(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        })
        .collect()
}

pub(crate) fn declared_item_ty_expr(tcx: TyCtxt<'_>, def_id: DefId) -> Option<TypeExpr> {
    let local = def_id.as_local()?;
    let node = tcx.hir_node_by_def_id(local);
    let ty = match node {
        rustc_hir::Node::Item(item) => match &item.kind {
            rustc_hir::ItemKind::Const(_, _, ty, _) => Some(*ty),
            rustc_hir::ItemKind::Static(_, _, ty, _) => Some(*ty),
            rustc_hir::ItemKind::TyAlias(_, _, ty) => Some(*ty),
            _ => None,
        },
        rustc_hir::Node::ImplItem(ii) => match &ii.kind {
            rustc_hir::ImplItemKind::Const(ty, _) => Some(*ty),
            rustc_hir::ImplItemKind::Type(ty) => Some(*ty),
            _ => None,
        },
        rustc_hir::Node::TraitItem(ti) => match &ti.kind {
            rustc_hir::TraitItemKind::Const(ty, _, _) => Some(*ty),
            rustc_hir::TraitItemKind::Type(_, Some(ty)) => Some(*ty),
            _ => None,
        },
        rustc_hir::Node::Field(field) => Some(field.ty),
        _ => None,
    }?;
    let sm = tcx.sess.source_map();
    let snippet = sm.span_to_snippet(ty.span).ok()?.trim().to_string();
    if snippet.is_empty() {
        None
    } else {
        Some(TypeExpr::Path(snippet))
    }
}

fn declared_fn_param_types(tcx: TyCtxt<'_>, def_id: DefId) -> Option<Vec<TypeExpr>> {
    let local = def_id.as_local()?;
    let decl = match tcx.hir_node_by_def_id(local) {
        rustc_hir::Node::Item(item) => match &item.kind {
            rustc_hir::ItemKind::Fn { sig, .. } => Some(sig.decl),
            _ => None,
        },
        rustc_hir::Node::ImplItem(ii) => match &ii.kind {
            rustc_hir::ImplItemKind::Fn(sig, _) => Some(sig.decl),
            _ => None,
        },
        rustc_hir::Node::TraitItem(ti) => match &ti.kind {
            rustc_hir::TraitItemKind::Fn(sig, _) => Some(sig.decl),
            _ => None,
        },
        _ => None,
    }?;
    let sm = tcx.sess.source_map();
    Some(decl.inputs.iter().map(|ty| sm.span_to_snippet(ty.span).ok().map(|s| TypeExpr::Path(s.trim().to_string())).unwrap_or(TypeExpr::Path("_".to_string()))).collect())
}

pub(crate) fn declared_fn_return_type_expr(tcx: TyCtxt<'_>, def_id: DefId) -> Option<TypeExpr> {
    let local = def_id.as_local()?;
    let output = match tcx.hir_node_by_def_id(local) {
        rustc_hir::Node::Item(item) => match &item.kind {
            rustc_hir::ItemKind::Fn { sig, .. } => Some(sig.decl.output),
            _ => None,
        },
        rustc_hir::Node::ImplItem(ii) => match &ii.kind {
            rustc_hir::ImplItemKind::Fn(sig, _) => Some(sig.decl.output),
            _ => None,
        },
        rustc_hir::Node::TraitItem(ti) => match &ti.kind {
            rustc_hir::TraitItemKind::Fn(sig, _) => Some(sig.decl.output),
            _ => None,
        },
        _ => None,
    }?;
    match output {
        rustc_hir::FnRetTy::DefaultReturn(_) => Some(TypeExpr::Primitive(PrimType::Unit)),
        rustc_hir::FnRetTy::Return(ty) => {
            let sm = tcx.sess.source_map();
            let snippet = sm.span_to_snippet(ty.span).ok()?.trim().to_string();
            if snippet.is_empty() {
                None
            } else {
                Some(TypeExpr::Path(snippet))
            }
        }
    }
}

pub(crate) fn lower_ty<'tcx>(tcx: TyCtxt<'tcx>, ty: ty::Ty<'tcx>) -> TypeExpr {
    match ty.kind() {
        ty::TyKind::Bool => TypeExpr::Primitive(PrimType::Bool),
        ty::TyKind::Char => TypeExpr::Primitive(PrimType::Char),
        ty::TyKind::Str => TypeExpr::Primitive(PrimType::Str),
        ty::TyKind::Int(i) => TypeExpr::Primitive(match i {
            ty::IntTy::Isize => PrimType::Isize,
            ty::IntTy::I8 => PrimType::I8,
            ty::IntTy::I16 => PrimType::I16,
            ty::IntTy::I32 => PrimType::I32,
            ty::IntTy::I64 => PrimType::I64,
            ty::IntTy::I128 => PrimType::I128,
        }),
        ty::TyKind::Uint(u) => TypeExpr::Primitive(match u {
            ty::UintTy::Usize => PrimType::Usize,
            ty::UintTy::U8 => PrimType::U8,
            ty::UintTy::U16 => PrimType::U16,
            ty::UintTy::U32 => PrimType::U32,
            ty::UintTy::U64 => PrimType::U64,
            ty::UintTy::U128 => PrimType::U128,
        }),
        ty::TyKind::Float(f) => TypeExpr::Primitive(match f {
            ty::FloatTy::F32 => PrimType::F32,
            ty::FloatTy::F64 => PrimType::F64,
            ty::FloatTy::F16 => return TypeExpr::Path("f16".to_string()),
            ty::FloatTy::F128 => return TypeExpr::Path("f128".to_string()),
        }),
        ty::TyKind::Never => TypeExpr::Primitive(PrimType::Never),
        ty::TyKind::Tuple(items) => {
            if items.is_empty() {
                TypeExpr::Primitive(PrimType::Unit)
            } else {
                TypeExpr::Tuple(items.iter().map(|t| lower_ty(tcx, t)).collect())
            }
        }
        ty::TyKind::Ref(region, inner, mutbl) => TypeExpr::Ref {
            lifetime: match region.kind() {
                ty::RegionKind::ReStatic => Some("'static".to_string()),
                _ => None,
            },
            inner: Box::new(lower_ty(tcx, *inner)),
            mutable: matches!(mutbl, rustc_hir::Mutability::Mut),
        },
        ty::TyKind::RawPtr(inner_ty, mutbl) => TypeExpr::RawPtr { inner: Box::new(lower_ty(tcx, *inner_ty)), mutable: matches!(mutbl, rustc_hir::Mutability::Mut) },
        ty::TyKind::Array(inner, len) => TypeExpr::Array { inner: Box::new(lower_ty(tcx, *inner)), len: len.try_to_target_usize(tcx) },
        ty::TyKind::Slice(inner) => TypeExpr::Slice(Box::new(lower_ty(tcx, *inner))),
        ty::TyKind::FnPtr(sig, _) => {
            let sig = sig.skip_binder();
            let params = sig.inputs().iter().map(|t| lower_ty(tcx, *t)).collect();
            let ret = Box::new(lower_ty(tcx, sig.output()));
            TypeExpr::FnPtr { params, ret }
        }
        ty::TyKind::FnDef(def_id, args) => {
            let sig = tcx.fn_sig(*def_id).instantiate(tcx, args).skip_binder();
            let params = sig.inputs().iter().map(|t| lower_ty(tcx, *t)).collect();
            let ret = Box::new(lower_ty(tcx, sig.output()));
            TypeExpr::FnPtr { params, ret }
        }
        ty::TyKind::Closure(def_id, args) => {
            let sig = args.as_closure().sig().skip_binder();
            let params = sig.inputs().iter().map(|t| lower_ty(tcx, *t)).collect();
            let ret = Box::new(lower_ty(tcx, sig.output()));
            TypeExpr::FnPtr { params, ret }
        }
        ty::TyKind::Adt(adt, args) => {
            let base = norm::path(tcx, adt.did());
            let mut lowered_args: Vec<TypeExpr> = Vec::new();
            lowered_args.extend(args.regions().map(|region| match region.kind() {
                ty::RegionKind::ReStatic => TypeExpr::Path("'static".to_string()),
                _ => TypeExpr::Path("'static".to_string()),
            }));
            lowered_args.extend(args.types().map(|ty| lower_ty(tcx, ty)));
            lowered_args.extend(args.consts().map(|_| TypeExpr::Path("_".to_string())));
            if lowered_args.is_empty() {
                TypeExpr::Path(base)
            } else {
                TypeExpr::AppliedPath { base, args: lowered_args }
            }
        }
        ty::TyKind::Param(_param) => {
            // Structural fallback: avoid emitting unresolved generic parameters (e.g., `T`)
            // into rendered output, since emitted crates may not carry the original
            // generic bindings. Collapse to `_` to preserve well-formedness.
            TypeExpr::Path("_".to_string())
        }
        ty::TyKind::Dynamic(preds, _) => {
            let principal = preds.principal_def_id().map(|did| norm::path(tcx, did)).unwrap_or_else(|| panic!("unsupported dyn type without principal trait: {ty:?}"));
            TypeExpr::DynTrait(principal)
        }
        ty::TyKind::Coroutine(_, args) => {
            let ret = args.as_coroutine().return_ty();
            lower_ty(tcx, ret)
        }
        ty::TyKind::Alias(kind, alias_ty) => {
            if matches!(kind, ty::AliasTyKind::Opaque) {
                let hidden = tcx.type_of(alias_ty.def_id).instantiate_identity();
                if matches!(hidden.kind(), ty::TyKind::Alias(_, inner) if inner.def_id == alias_ty.def_id) {
                    panic!("opaque alias resolved to itself: {:?}", alias_ty.def_id);
                }
                lower_ty(tcx, hidden)
            } else {
                TypeExpr::Path(norm::path(tcx, alias_ty.def_id))
            }
        }
        _ => panic!("unsupported structural type variant: {ty:?}"),
    }
}

pub(crate) fn render_type_expr(_tcx: TyCtxt<'_>, expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(p) => match p {
            PrimType::Bool => "bool".to_string(),
            PrimType::Char => "char".to_string(),
            PrimType::Str => "str".to_string(),
            PrimType::U8 => "u8".to_string(),
            PrimType::U16 => "u16".to_string(),
            PrimType::U32 => "u32".to_string(),
            PrimType::U64 => "u64".to_string(),
            PrimType::U128 => "u128".to_string(),
            PrimType::Usize => "usize".to_string(),
            PrimType::I8 => "i8".to_string(),
            PrimType::I16 => "i16".to_string(),
            PrimType::I32 => "i32".to_string(),
            PrimType::I64 => "i64".to_string(),
            PrimType::I128 => "i128".to_string(),
            PrimType::Isize => "isize".to_string(),
            PrimType::F32 => "f32".to_string(),
            PrimType::F64 => "f64".to_string(),
            PrimType::Unit => "()".to_string(),
            PrimType::Never => "!".to_string(),
        },
        TypeExpr::Ref { lifetime, inner, mutable } => {
            let mut out = String::from("&");
            if let Some(lf) = lifetime {
                out.push_str(lf);
                out.push(' ');
            }
            if *mutable {
                out.push_str("mut ");
            }
            out.push_str(&render_type_expr(_tcx, inner));
            out
        }
        TypeExpr::RawPtr { inner, mutable } => {
            if *mutable {
                format!("*mut {}", render_type_expr(_tcx, inner))
            } else {
                format!("*const {}", render_type_expr(_tcx, inner))
            }
        }
        TypeExpr::Array { inner, len } => match len {
            Some(len) => format!("[{}; {len}]", render_type_expr(_tcx, inner)),
            None => format!("[{}; _]", render_type_expr(_tcx, inner)),
        },
        TypeExpr::Slice(inner) => format!("[{}]", render_type_expr(_tcx, inner)),
        TypeExpr::Tuple(items) => {
            if items.is_empty() {
                "()".to_string()
            } else {
                let mut rendered = items.iter().map(|i| render_type_expr(_tcx, i)).collect::<Vec<_>>();
                if rendered.len() == 1 {
                    rendered[0].push(',');
                }
                format!("({})", rendered.join(", "))
            }
        }
        TypeExpr::FnPtr { params, ret } => {
            let params = params.iter().map(|p| render_type_expr(_tcx, p)).collect::<Vec<_>>().join(", ");
            format!("fn({params}) -> {}", render_type_expr(_tcx, ret))
        }
        TypeExpr::Param(name) => name.clone(),
        TypeExpr::DynTrait(path) => format!("dyn {path}"),
        TypeExpr::ImplTrait(path) => format!("impl {path}"),
        TypeExpr::AppliedPath { base, args } => {
            // Filter implicit allocator parameters like `std::alloc::Global`
            // which appear in types such as `Vec<T, A>` but should not be
            // rendered in emitted Rust.
            let rendered_args: Vec<String> = args
                .iter()
                .filter(|a| match a {
                    TypeExpr::Path(p) => !(p.contains("std::alloc::Global") || p.contains("alloc::Global")),
                    _ => true,
                })
                .map(|a| render_type_expr(_tcx, a))
                .collect();

            if rendered_args.is_empty() {
                base.clone()
            } else {
                format!("{base}::<{}>", rendered_args.join(", "))
            }
        }
        TypeExpr::Path(path) => path.clone(),
    }
}

/// Deterministically construct a default return expression string for a TypeExpr.
/// This must never panic and must not introduce suppressed bindings.
pub(crate) fn default_return_expr(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(PrimType::Unit) => "()".to_string(),
        TypeExpr::Primitive(PrimType::Bool) => "false".to_string(),
        TypeExpr::Primitive(PrimType::Char) => "'\\0'".to_string(),
        TypeExpr::Primitive(PrimType::Str) => "\"\"".to_string(),
        TypeExpr::Primitive(PrimType::Never) => "loop {}".to_string(),
        TypeExpr::Primitive(_) => "0".to_string(),
        TypeExpr::Tuple(items) => {
            if items.is_empty() {
                "()".to_string()
            } else {
                let inner = items.iter().map(default_return_expr).collect::<Vec<_>>().join(", ");
                format!("({inner})")
            }
        }
        TypeExpr::Ref { .. } => "&()".to_string(),
        TypeExpr::RawPtr { .. } => "std::ptr::null()".to_string(),
        TypeExpr::Array { inner, len } => match len {
            Some(n) => {
                let elem = default_return_expr(inner);
                format!("[{elem}; {n}]")
            }
            None => "()".to_string(),
        },
        TypeExpr::Slice(_) => "&[]".to_string(),
        TypeExpr::FnPtr { .. } => "()".to_string(),
        TypeExpr::Param(_) => "()".to_string(),
        TypeExpr::DynTrait(_) => "()".to_string(),
        TypeExpr::ImplTrait(_) => "()".to_string(),
        TypeExpr::AppliedPath { base, .. } => {
            if base.ends_with("Vec") {
                "Vec::<_>::new()".to_string()
            } else if base.ends_with("String") {
                "String::new()".to_string()
            } else if base.ends_with("Option") {
                "None".to_string()
            } else if base.ends_with("Symbol") {
                // Non-Default ADTs must not fall back to Default::default().
                // Emit empty Vec for common extractor pattern Vec<Symbol>.
                "Vec::<_>::new()".to_string()
            } else {
                "()".to_string()
            }
        }
        TypeExpr::Path(p) => {
            if p.contains("Vec<") || p.ends_with("Vec") {
                "Vec::<_>::new()".to_string()
            } else if p.ends_with("String") {
                "String::new()".to_string()
            } else if p.contains("Option<") || p.ends_with("Option") {
                "None".to_string()
            } else if p.ends_with("Symbol") {
                // Common extractor pattern: Vec<Symbol> lowered via path fallback
                "Vec::<_>::new()".to_string()
            } else {
                "()".to_string()
            }
        }
    }
}

/// Compiler-internal traits emitted as side-effect impls by builtin derives.
/// These are never user-writable macros and must never appear in #[derive(...)].
/// Sources:
///   - StructuralPartialEq: rustc_builtin_macros/deriving/cmp/partial_eq.rs (bonus impl from derive(PartialEq))
///   - TrivialClone:        rustc_builtin_macros/deriving/clone.rs           (bonus impl from derive(Clone))
const INTERNAL_DERIVE_TRAITS: &[&str] = &["StructuralPartialEq", "TrivialClone"];

/// Extract derive macro names from automatically_derived impls.
/// Filters out compiler-internal side-effect traits that are not user-writable macros.
pub(crate) fn collect_derives(tcx: TyCtxt<'_>, def_id: DefId) -> Vec<String> {
    let mut derives = Vec::new();
    for impl_did in tcx.all_local_trait_impls(()).values().flatten().copied() {
        let impl_def_id = impl_did.to_def_id();
        // Only impls for our ADT.
        let Some(adt) = tcx.type_of(impl_def_id).instantiate_identity().ty_adt_def() else { continue };
        if adt.did() != def_id {
            continue;
        }
        // Must be #[automatically_derived] with a Derive expansion context.
        if !tcx.is_automatically_derived(impl_def_id) {
            continue;
        }
        let outer = tcx.def_span(impl_did).ctxt().outer_expn_data();
        if !matches!(outer.kind, ExpnKind::Macro(MacroKind::Derive, _)) {
            continue;
        }
        // Extract the trait name.
        let trait_ref = tcx.impl_trait_ref(impl_def_id);
        let trait_did = trait_ref.skip_binder().def_id;
        let name = norm::short(&norm::path(tcx, trait_did)).to_string();
        // Skip compiler-internal side-effect traits.
        if INTERNAL_DERIVE_TRAITS.contains(&name.as_str()) {
            continue;
        }
        derives.push(name);
    }
    derives
}
