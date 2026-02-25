use model::ir::node::{Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, StructKind, Visibility};
use rustc_hir::{def::DefKind, PatKind, Safety};
use rustc_middle::ty::print::PrintTraitRefExt;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;

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
            let for_trait = tcx.impl_opt_trait_ref(def_id).map(|eb| tcx.def_path_str(eb.skip_binder().def_id));
            NodeKind::Impl { for_struct: name.clone(), for_trait, generics, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false }
        }
        DefKind::Fn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Function { name: name.clone(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Method { name: name.clone(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::Const => NodeKind::Const { name: name.clone(), vis, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), value: String::new(), attrs: Vec::new() },
        DefKind::Static { .. } => NodeKind::Static { name: name.clone(), vis, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), value: String::new(), mutable: true, attrs: Vec::new() },
        DefKind::TyAlias => NodeKind::TypeAlias { name: name.clone(), vis, generics, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), attrs: Vec::new(), where_clauses: Vec::new() },
        DefKind::Use => {
            // Re-export / use item — walk HIR to get path and alias.
            if let Some(local) = def_id.as_local() {
                if let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) {
                    if let rustc_hir::ItemKind::Use(use_path, use_kind) = item.kind {
                        let path = use_path.res.iter().find_map(|r| if let Some(rustc_hir::def::Res::Def(_, did)) = r { Some(tcx.def_path_str(*did)) } else { None }).unwrap_or_else(|| name.clone());
                        let glob = matches!(use_kind, rustc_hir::UseKind::Glob);
                        let alias = match use_kind {
                            rustc_hir::UseKind::Single(ident) if ident.name.as_str() != path.rsplit("::").next().unwrap_or("") => Some(ident.to_string()),
                            _ => None,
                        };
                        return Some(Node { id, kind: NodeKind::Use { vis, path, alias, glob }, span: Some(span_str) });
                    }
                }
            }
            return None;
        }
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
    // Not all DefKinds support generics_of — guard before calling it.
    let supported = matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias);
    if !supported {
        return Vec::new();
    }

    let gens = tcx.generics_of(def_id);

    let mut bounds_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    if matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias) {
        for (clause, _span) in tcx.predicates_of(def_id).predicates {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(tp) => {
                    if let ty::TyKind::Param(p) = tp.self_ty().kind() {
                        let trait_str = format!("{}", tp.trait_ref.print_only_trait_name());
                        bounds_map.entry(p.name.to_string()).or_default().push(trait_str);
                    }
                }
                ty::ClauseKind::RegionOutlives(ro) => {
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

/// Map function params: use HIR body param patterns for real names,
/// fall back to `p{i}` for non-local defs or abstract items.
fn map_params(tcx: TyCtxt<'_>, def_id: DefId, inputs: &[ty::Ty<'_>]) -> Vec<Param> {
    // Try to get HIR param names from the body.
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
            Param { name, ty: fmt_ty(tcx, *ty), is_self, mutable: false, lifetime: None }
        })
        .collect()
}

fn map_fields<'a, I>(tcx: TyCtxt<'a>, fields: I) -> Vec<Field>
where I: Iterator<Item = &'a ty::FieldDef> {
    fields
        .map(|f| {
            // Use tcx.visibility for real field visibility instead of always Public.
            let vis = map_vis(tcx.visibility(f.did));
            Field { name: Some(f.name.to_string()), ty: fmt_ty(tcx, tcx.type_of(f.did).instantiate_identity()), vis }
        })
        .collect()
}

fn fmt_ty(tcx: TyCtxt<'_>, ty: ty::Ty<'_>) -> String {
    ty.to_string()
}
