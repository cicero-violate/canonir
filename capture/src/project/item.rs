use model::ir::node::TraitMethod;
use model::ir::node::{Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, StructKind, Visibility};
use rustc_hir::{def::DefKind, GenericBound, PatKind, PredicateOrigin, Safety, WherePredicateKind};
use rustc_span::hygiene::{ExpnKind, MacroKind};
use rustc_middle::ty::print::PrintTraitRefExt;
use rustc_middle::ty::AssocKind;
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
    let (generics, where_clauses) = map_generics(tcx, def_id);

    let kind = match tcx.def_kind(def_id) {
        DefKind::Mod => {
            let file = norm::module_file(tcx, def_id);
            // An inline module lives in the same file as its declaration span.
            let decl_file = norm::file(tcx, raw_span);
            let inline = file == decl_file && def_id.as_local().map_or(false, |local| {
                if let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) {
                    matches!(item.kind, rustc_hir::ItemKind::Mod(_, _))
                } else {
                    false
                }
            });
            NodeKind::Module { path: norm::module_path(tcx, def_id), file, vis, inline }
        }
        DefKind::Struct | DefKind::Union => {
            let adt = tcx.adt_def(def_id);
            let variant = adt.non_enum_variant();
            let struct_kind = match variant.ctor_kind() {
                Some(rustc_hir::def::CtorKind::Fn) => StructKind::Tuple,
                Some(rustc_hir::def::CtorKind::Const) => StructKind::Unit,
                _ => StructKind::Named,
            };
            let fields = map_fields(tcx, variant.fields.iter(), false);
            let derives = collect_derives(tcx, def_id);
            NodeKind::Struct { name, vis, generics, fields, derives, attrs: Vec::new(), where_clauses, struct_kind }
        }
        DefKind::Enum => {
            let adt = tcx.adt_def(def_id);
            let variants = adt.variants().iter().map(|v| EnumVariant { name: v.name.to_string(), fields: map_fields(tcx, v.fields.iter(), true) }).collect();
            let derives = collect_derives(tcx, def_id);
            NodeKind::Enum { name, vis, generics, variants, derives, attrs: Vec::new(), where_clauses }
        }
        DefKind::Trait => {
            let methods = collect_trait_methods(tcx, def_id);
            NodeKind::Trait { name, vis, generics, methods, attrs: Vec::new(), where_clauses, unsafe_: false }
        }
        DefKind::Impl { .. } => {
            let for_trait = tcx.impl_opt_trait_ref(def_id).map(|eb| norm::path(tcx, eb.skip_binder().def_id)).map(|p| norm::short(&p).to_string());
            let for_struct = tcx.type_of(def_id).instantiate_identity().ty_adt_def().map(|adt| norm::short(&norm::path(tcx, adt.did())).to_string()).unwrap_or_else(|| name.clone());
            NodeKind::Impl { for_struct, for_trait, generics, attrs: Vec::new(), where_clauses, unsafe_: false }
        }
        DefKind::Fn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let async_ = tcx.asyncness(def_id).is_async();
            let ret_raw = fmt_ty(tcx, sig.output().skip_binder());
            let ret = if async_ { unwrap_future_output(&ret_raw) } else { ret_raw };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let body = hir_body_src(tcx, def_id);
            NodeKind::Function { name, vis, generics, params, ret, body, attrs: Vec::new(), where_clauses, unsafe_, async_ }
        }
        DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let async_ = tcx.asyncness(def_id).is_async();
            let ret_raw = fmt_ty(tcx, sig.output().skip_binder());
            let ret = if async_ { unwrap_future_output(&ret_raw) } else { ret_raw };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let body = hir_body_src(tcx, def_id);
            NodeKind::Method { name, vis, generics, params, ret, body, attrs: Vec::new(), where_clauses, unsafe_, async_ }
        }
        DefKind::Const => {
            let ty_str = fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity());
            let value = hir_init_src(tcx, def_id);
            NodeKind::Const { name, vis, ty: ty_str, value, attrs: Vec::new() }
        }
        DefKind::Static { mutability, .. } => {
            let ty_str = fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity());
            let value = hir_init_src(tcx, def_id);
            let mutable = mutability == rustc_hir::Mutability::Mut;
            NodeKind::Static { name, vis, ty: ty_str, value, mutable, attrs: Vec::new() }
        }
        DefKind::TyAlias => NodeKind::TypeAlias { name, vis, generics, ty: fmt_ty(tcx, tcx.type_of(def_id).instantiate_identity()), attrs: Vec::new(), where_clauses },
        DefKind::Use => {
            if let Some(local) = def_id.as_local() {
                if let rustc_hir::Node::Item(item) = tcx.hir_node_by_def_id(local) {
                    if let rustc_hir::ItemKind::Use(use_path, use_kind) = item.kind {
                        let sm = tcx.sess.source_map();
                        let mut path = sm
                            .span_to_snippet(use_path.span)
                            .ok()
                            .map(|s| s.trim().trim_start_matches("::").to_string())
                            .filter(|s| !s.is_empty());

                        if path.is_none() {
                            path = use_path.res.iter().find_map(|r| {
                                if let Some(rustc_hir::def::Res::Def(_, did)) = r {
                                    Some(norm::path(tcx, *did))
                                } else {
                                    None
                                }
                            });
                        }

                        let path = path?;
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

/// Returns `(generic_params, where_clause_strings)`.
///
/// Inline bounds: read from HIR `GenericParam.bounds` (written as `<T: Bound>`).
/// Where clauses: read from HIR `Generics.predicates` with `origin == WhereClause`.
/// Both are source snippets — exactly what the user wrote, no path normalization.
fn map_generics(tcx: TyCtxt<'_>, def_id: DefId) -> (Vec<GenericParam>, Vec<String>) {
    let supported = matches!(
        tcx.def_kind(def_id),
        DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum
            | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias
    );
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
    let params: Vec<GenericParam> = hgen.params.iter()
        .filter(|p| {
            let n = p.name.ident().to_string();
            n != "Self" && !p.is_elided_lifetime() && !p.is_impl_trait()
        })
        .map(|p| {
            let name = p.name.ident().to_string();
            let is_lifetime = matches!(p.kind, rustc_hir::GenericParamKind::Lifetime { .. });
            // All bounds (inline and where) live in hgen.predicates since PR #93803.
            // Inline bounds have origin == GenericParam; where-clause bounds have origin == WhereClause.
            let bounds: Vec<String> = hgen.predicates.iter()
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
                let rhs: Vec<String> = bp.bounds.iter()
                    .filter_map(|b| sm.span_to_snippet(b.span()).ok())
                    .collect();
                if !lhs.is_empty() && !rhs.is_empty() {
                    where_clauses.push(format!("{}: {}", lhs, rhs.join(" + ")));
                }
            }
            WherePredicateKind::RegionPredicate(rp) => {
                let lhs = rp.lifetime.ident.to_string();
                let rhs: Vec<String> = rp.bounds.iter()
                    .filter_map(|b| sm.span_to_snippet(b.span()).ok())
                    .collect();
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
    let params = gens.own_params
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
            let ty_str = if is_self {
                let raw = fmt_ty(tcx, *ty);
                if raw.starts_with('&') { "&Self".to_string() } else { "Self".to_string() }
            } else {
                fmt_ty(tcx, *ty)
            };
            Param { name, ty: ty_str, is_self, mutable: false, lifetime: None }
        })
        .collect()
}

fn map_fields<'a, I>(tcx: TyCtxt<'a>, fields: I, in_enum: bool) -> Vec<Field>
where I: Iterator<Item = &'a ty::FieldDef> {
    fields
        .map(|f| {
            let vis = if in_enum { Visibility::Private } else { map_vis(tcx.visibility(f.did)) };
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
    let s = norm::ty_clean_impl(&s);
    norm::ty_strip_static_lifetime(&s)
}

/// For trait method declarations, HIR param names come from the TraitItem fn decl.
fn map_trait_method_params(tcx: TyCtxt<'_>, def_id: DefId, inputs: &[ty::Ty<'_>]) -> Vec<Param> {
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
            let name = if is_self {
                "self".to_string()
            } else {
                hir_names.get(i).and_then(|n| n.clone()).unwrap_or_else(|| format!("p{i}"))
            };
            let ty_str = if is_self {
                let raw = fmt_ty(tcx, *ty);
                if raw.starts_with('&') { "&Self".to_string() } else { "Self".to_string() }
            } else {
                fmt_ty(tcx, *ty)
            };
            Param { name, ty: ty_str, is_self, mutable: false, lifetime: None }
        })
        .collect()
}

/// Unwrap `impl Future<Output = T>` → `T` for async fn return types.
fn unwrap_future_output(ret: &str) -> String {
    let trimmed = ret.trim();
    if let Some(inner) = trimmed.strip_prefix("impl Future<Output") {
        let inner = inner.trim_start_matches(|c: char| c.is_whitespace());
        if let Some(inner) = inner.strip_prefix("=") {
            let inner = inner.trim();
            if let Some(t) = inner.strip_suffix('>') {
                return t.trim().to_string();
            }
        }
    }
    ret.to_string()
}

/// Slice the source text of a Const or Static initializer expression.
fn hir_init_src(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let Some(local) = def_id.as_local() else { return String::new() };
    let Some(body) = tcx.hir_maybe_body_owned_by(local) else { return String::new() };
    let span = body.value.span;
    let sm = tcx.sess.source_map();
    sm.span_to_snippet(span).unwrap_or_default()
}

/// Collect TraitMethod entries for a trait DefId from associated_items.
fn collect_trait_methods(tcx: TyCtxt<'_>, trait_def_id: DefId) -> Vec<TraitMethod> {
    tcx.associated_items(trait_def_id)
        .in_definition_order()
        .filter(|item| matches!(item.kind, AssocKind::Fn { .. }))
        .map(|item| {
            let sig = tcx.fn_sig(item.def_id).skip_binder();
            let params = map_trait_method_params(tcx, item.def_id, sig.inputs().skip_binder());
            let ret = fmt_ty(tcx, sig.output().skip_binder());
            let ret = if tcx.asyncness(item.def_id).is_async() { unwrap_future_output(&ret) } else { ret };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(item.def_id).is_async();
            let (generics, _where_clauses) = map_generics(tcx, item.def_id);
            let vis = map_vis(tcx.visibility(item.def_id));
            TraitMethod { name: item.name().to_string(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        })
        .collect()
}

/// Capture function/method body as Body::Raw(source) via HIR body span.
fn hir_body_src(tcx: TyCtxt<'_>, def_id: DefId) -> Body {
    let Some(local) = def_id.as_local() else { return Body::None };
    let Some(body) = tcx.hir_maybe_body_owned_by(local) else { return Body::None };
    let span = body.value.span;
    let sm = tcx.sess.source_map();
    match sm.span_to_snippet(span) {
        Ok(src) => Body::Raw(strip_outer_braces(src)),
        Err(_) => Body::None,
    }
}

/// Strip outer `{ ... }` braces and dedent.
fn strip_outer_braces(src: String) -> String {
    let trimmed = src.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let lines: Vec<&str> = inner.lines().collect();
        let min_indent = lines.iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        let dedented: Vec<&str> = lines.iter()
            .map(|l| if l.len() >= min_indent { &l[min_indent..] } else { l.trim_start() })
            .collect();
        dedented.join("\n").trim().to_string()
    } else {
        src
    }
}

/// Compiler-internal traits emitted as side-effect impls by builtin derives.
/// These are never user-writable macros and must never appear in #[derive(...)].
/// Sources:
///   - StructuralPartialEq: rustc_builtin_macros/deriving/cmp/partial_eq.rs (bonus impl from derive(PartialEq))
///   - TrivialClone:        rustc_builtin_macros/deriving/clone.rs           (bonus impl from derive(Clone))
const INTERNAL_DERIVE_TRAITS: &[&str] = &[
    "StructuralPartialEq",
    "TrivialClone",
];

/// Extract derive macro names from automatically_derived impls.
/// Filters out compiler-internal side-effect traits that are not user-writable macros.
fn collect_derives(tcx: TyCtxt<'_>, def_id: DefId) -> Vec<String> {
    let mut derives = Vec::new();
    for impl_did in tcx.all_local_trait_impls(()).values().flatten().copied() {
        let impl_def_id = impl_did.to_def_id();
        // Only impls for our ADT.
        let Some(adt) = tcx.type_of(impl_def_id).instantiate_identity().ty_adt_def() else { continue };
        if adt.did() != def_id { continue; }
        // Must be #[automatically_derived] with a Derive expansion context.
        if !tcx.is_automatically_derived(impl_def_id) { continue; }
        let outer = tcx.def_span(impl_did).ctxt().outer_expn_data();
        if !matches!(outer.kind, ExpnKind::Macro(MacroKind::Derive, _)) { continue; }
        // Extract the trait name.
        let trait_ref = tcx.impl_trait_ref(impl_def_id);
        let trait_did = trait_ref.skip_binder().def_id;
        let name = norm::short(&norm::path(tcx, trait_did)).to_string();
        // Skip compiler-internal side-effect traits.
        if INTERNAL_DERIVE_TRAITS.contains(&name.as_str()) { continue; }
        derives.push(name);
    }
    derives
}
