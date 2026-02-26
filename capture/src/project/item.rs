use model::ir::node::{Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, StructKind, Visibility};
use rustc_hir::{def::DefKind, PatKind, Safety};
use rustc_middle::ty::print::PrintTraitRefExt;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;

use crate::index::Index;
use crate::norm;

/// Structural projection: DefId -> NodeKind using HIR/ty queries.
/// All strings are canonicalized via norm:: before NodeKind construction.
pub fn project_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<Node> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let raw_span = tcx.def_span(def_id);
    let span_str = norm::span(tcx, raw_span);
    let file_str = norm::file(tcx, raw_span);

    let vis = map_vis(tcx.visibility(def_id));
    let generics = map_generics(tcx, def_id);

    let kind = match tcx.def_kind(def_id) {
        DefKind::Mod => NodeKind::Module { path: norm::module_path(tcx, def_id), file: norm::module_file(tcx, def_id), inline: false },
        DefKind::Struct | DefKind::Union => {
            let adt = tcx.adt_def(def_id);
            let variant = adt.non_enum_variant();
            let struct_kind = match variant.ctor_kind() {
                Some(rustc_hir::def::CtorKind::Fn) => StructKind::Tuple,
                Some(rustc_hir::def::CtorKind::Const) => StructKind::Unit,
                _ => StructKind::Named,
            };
            let fields = map_fields(tcx, variant.fields.iter(), false);
            NodeKind::Struct { name, vis, generics, fields, derives: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new(), struct_kind }
        }
        DefKind::Enum => {
            let adt = tcx.adt_def(def_id);
            let variants = adt.variants().iter().map(|v| EnumVariant { name: v.name.to_string(), fields: map_fields(tcx, v.fields.iter(), true) }).collect();
            NodeKind::Enum { name, vis, generics, variants, derives: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new() }
        }
        DefKind::Trait => NodeKind::Trait { name, vis, generics, methods: Vec::new(), attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false },
        DefKind::Impl { .. } => {
            let for_trait = tcx.impl_opt_trait_ref(def_id).map(|eb| norm::path(tcx, eb.skip_binder().def_id)).map(|p| norm::short(&p).to_string());
            let for_struct = tcx.type_of(def_id).instantiate_identity().ty_adt_def().map(|adt| norm::short(&norm::path(tcx, adt.did())).to_string()).unwrap_or_else(|| name.clone());
            NodeKind::Impl { for_struct, for_trait, generics, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false }
        }
        DefKind::Fn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Function { name, vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(def_id).is_async();
            NodeKind::Method { name, vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::Const => NodeKind::Const { name, vis, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), value: String::new(), attrs: Vec::new() },
        DefKind::Static { .. } => NodeKind::Static { name, vis, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), value: String::new(), mutable: true, attrs: Vec::new() },
        DefKind::TyAlias => NodeKind::TypeAlias { name, vis, generics, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), attrs: Vec::new(), where_clauses: Vec::new() },
        DefKind::Use => {
            if let Some(local) = def_id.as_local() {
                if let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) {
                    if let rustc_hir::ItemKind::Use(use_path, use_kind) = item.kind {
                        let path = use_path.res.iter().find_map(|r| {
                            if let Some(rustc_hir::def::Res::Def(_, did)) = r {
                                let p = norm::path(tcx, *did);
                                // Only emit canonical paths:
                                // - local items must be "crate::..."
                                // - external items must be "some_crate::..."
                                let is_local = p.starts_with("crate::");
                                let is_external = p.contains("::") && !p.starts_with("crate");
                                if is_local || is_external {
                                    Some(p)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        });
                        // If resolution failed (no Res::Def found), skip this Use node.
                        let path = match path {
                            Some(p) => p,
                            None => return None,
                        };
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

fn map_generics(tcx: TyCtxt<'_>, def_id: DefId) -> Vec<GenericParam> {
    let supported = matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias);
    if !supported {
        return Vec::new();
    }

    let gens = tcx.generics_of(def_id);
    let mut bounds_map: std::collections::HashMap<String, Vec<String>> = Default::default();

    for (clause, _span) in tcx.predicates_of(def_id).predicates {
        match clause.kind().skip_binder() {
            ty::ClauseKind::Trait(tp) => {
                if let ty::TyKind::Param(p) = tp.self_ty().kind() {
                    let trait_str = format!("{}", tp.trait_ref.print_only_trait_name());
                    // Skip implicit Sized bound — rustc adds it to every type param.
                    if trait_str == "Sized" || trait_str == "MetaSized" {
                        continue;
                    }
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

    gens.own_params
        .iter()
        .filter(|p| p.name.as_str() != "Self")
        .map(|p| {
            let name = p.name.to_string();
            let bounds = bounds_map.remove(&name).unwrap_or_default().into_iter().filter(|b| b != "Sized" && b != "MetaSized").collect();
            let is_lifetime = matches!(p.kind, ty::GenericParamDefKind::Lifetime);
            GenericParam { name, bounds, is_lifetime, default_ty: None }
        })
        .collect()
}

fn map_params(tcx: TyCtxt<'_>, def_id: DefId, inputs: &[ty::Ty<'_>]) -> Vec<Param> {
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

fn map_fields<'a, I>(tcx: TyCtxt<'a>, fields: I, in_enum: bool) -> Vec<Field>
where I: Iterator<Item = &'a ty::FieldDef> {
    fields
        .map(|f| {
            let vis = if in_enum { Visibility::Private } else { map_vis(tcx.visibility(f.did)) };
            // Tuple struct/variant fields have numeric names "0", "1", ... in HIR.
            // ModelIR uses name: None for unnamed (positional) fields.
            let raw_name = f.name.to_string();
            let name = if raw_name.chars().all(|c| c.is_ascii_digit()) { None } else { Some(raw_name) };
            Field { name, ty: fmt_ty(tcx, tcx.type_of(f.did).instantiate_identity()), vis }
        })
        .collect()
}

fn fmt_ty(tcx: TyCtxt<'_>, ty: ty::Ty<'_>) -> String {
    let krate = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE).to_string();
    let s = norm::ty(&ty.to_string());
    let s = norm::ty_strip_local(&s, &krate);
    norm::ty_clean_impl(&s)
}
