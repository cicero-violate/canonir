use model::ir::node::{Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, StructKind, Visibility};
use rustc_hir::{def::DefKind, Safety};
use rustc_middle::ty::{self, TyCtxt};
use rustc_middle::ty::print::PrintTraitRefExt;
use rustc_span::def_id::DefId;
use rustc_middle::mir;

use crate::index::Index;

/// Structural projection: DefId -> NodeKind using HIR/ty queries (projection only).
pub fn project_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<Node> {
    let id = *index.def_to_node.get(&def_id)?;
    let name = tcx.def_path_str(def_id);
    let span = tcx.def_span(def_id);
    let span_str = format!("{:?}", span);

    let vis = map_vis(tcx.visibility(def_id));
    let generics = map_generics(tcx, def_id);

    let kind = match tcx.def_kind(def_id) {
        DefKind::Mod => NodeKind::Module { path: name.clone(), file: span_str.clone(), inline: false },
        DefKind::Struct | DefKind::Union => {
            let adt = tcx.adt_def(def_id);
            let variant = adt.non_enum_variant();
            let struct_kind = match variant.ctor_kind() {
                Some(rustc_hir::def::CtorKind::Fn) => StructKind::Tuple,
                Some(rustc_hir::def::CtorKind::Const) => StructKind::Unit,
                _ => StructKind::Named,
            };
            let fields = map_fields(tcx, variant.fields.iter());
            NodeKind::Struct { name: name.clone(), vis, generics, fields, derives: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new(), struct_kind }
        }
        DefKind::Enum => {
            let adt = tcx.adt_def(def_id);
            let variants = adt.variants().iter().map(|v| EnumVariant { name: v.name.to_string(), fields: map_fields(tcx, v.fields.iter()) }).collect();
            NodeKind::Enum { name: name.clone(), vis, generics, variants, derives: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new() }
        }
        DefKind::Trait => NodeKind::Trait { name: name.clone(), vis, generics, methods: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false },
        DefKind::Impl { .. } => {
            // impl_opt_trait_ref returns None for inherent impls, Some for trait impls.
            let for_trait = tcx.impl_opt_trait_ref(def_id).map(|early_binder| tcx.def_path_str(early_binder.skip_binder().def_id));
            NodeKind::Impl { for_struct: name.clone(), for_trait, generics, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false }
        }
        DefKind::Fn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(sig.inputs().skip_binder(), tcx);
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Function { name: name.clone(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(sig.inputs().skip_binder(), tcx);
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Method { name: name.clone(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::Const => NodeKind::Const {
            name: name.clone(),
            vis,
            ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()),
            value: eval_const_value(tcx, def_id),
            attrs: Vec::new(),
        },
        DefKind::Static { .. } => NodeKind::Static {
            name: name.clone(),
            vis,
            ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()),
            value: eval_const_value(tcx, def_id),
            mutable: true,
            attrs: Vec::new(),
        },
        DefKind::TyAlias => NodeKind::TypeAlias { name: name.clone(), vis, generics, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), attrs: Vec::new(), where_clauses: Vec::new() },
        // All other DefKinds (TyParam, LifetimeParam, Variant, Field, Closure,
        // AnonConst, Ctor, etc.) are not top-level ModelIR items — skip them.
        _ => return None,
    };

    Some(Node { id, kind, span: Some(span_str) })
}

fn map_vis(v: ty::Visibility<DefId>) -> Visibility {
    if v.is_public() {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

/// Map generic params for `def_id`, pulling trait/lifetime bounds from
/// `predicates_of` and attaching them to the correct param by name.
fn map_generics(tcx: TyCtxt<'_>, def_id: DefId) -> Vec<GenericParam> {
    let gens = tcx.generics_of(def_id);

    // Collect bounds per param name from predicates_of.
    // predicates_of may ICE on some items (e.g. closures) — guard with a check.
    let mut bounds_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    if matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias) {
        for (clause, _span) in tcx.predicates_of(def_id).predicates {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(tp) => {
                    // Only collect bounds where the self type is a plain type param.
                    if let ty::TyKind::Param(p) = tp.self_ty().kind() {
                        let trait_str = format!("{}", tp.trait_ref.print_only_trait_name());
                        bounds_map.entry(p.name.to_string()).or_default().push(trait_str);
                    }
                }
                ty::ClauseKind::RegionOutlives(ro) => {
                    // 'a: 'b — attach to the lifetime param 'a.
                    let name = format!("{}", ro.0);
                    let bound = format!("{}", ro.1);
                    bounds_map.entry(name).or_default().push(bound);
                }
                _ => {}
            }
        }
    }

    gens.own_params
        .iter()
        .map(|p| {
            let name = p.name.to_string();
            let bounds = bounds_map.remove(&name).unwrap_or_default();
            let is_lifetime = matches!(p.kind, ty::GenericParamDefKind::Lifetime);
            GenericParam { name, bounds, is_lifetime, default_ty: None }
        })
        .collect()
}

fn map_params(inputs: &[ty::Ty<'_>], tcx: TyCtxt<'_>) -> Vec<Param> {
    inputs.iter().enumerate().map(|(i, ty)| Param { name: format!("p{i}"), ty: fmt_ty(tcx, *ty), is_self: false, mutable: false, lifetime: None }).collect()
}

fn map_fields<'a, I>(tcx: TyCtxt<'a>, fields: I) -> Vec<Field>
where I: Iterator<Item = &'a ty::FieldDef> {
    fields.map(|f| Field { name: Some(f.name.to_string()), ty: fmt_ty(tcx, tcx.type_of(f.did).instantiate_identity()), vis: Visibility::Public }).collect()
}

fn fmt_ty(tcx: TyCtxt<'_>, ty: ty::Ty<'_>) -> String {
    ty.to_string()
}

/// Attempt to evaluate a Const or Static item's value as a display string.
/// Returns empty string if evaluation fails.
fn eval_const_value(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let ty = tcx.type_of(def_id).instantiate_identity();
    match tcx.const_eval_poly(def_id) {
        Ok(val) => format!("{}", mir::Const::from_value(val, ty)),
        Err(_) => String::new(),
    }
}
