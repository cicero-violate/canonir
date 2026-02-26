use model::ir::node::TraitMethod;
use model::ir::node::{Body, EnumVariant, Field, GenericParam, Node, NodeKind, Param, StructKind, Visibility};
use rustc_hir::{def::DefKind, PatKind, Safety};
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
        DefKind::Trait => {
            let methods = collect_trait_methods(tcx, def_id);
            NodeKind::Trait { name, vis, generics, methods, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false }
        }
        DefKind::Impl { .. } => {
            let for_trait = tcx.impl_opt_trait_ref(def_id).map(|eb| norm::path(tcx, eb.skip_binder().def_id)).map(|p| norm::short(&p).to_string());
            let for_struct = tcx.type_of(def_id).instantiate_identity().ty_adt_def().map(|adt| norm::short(&norm::path(tcx, adt.did())).to_string()).unwrap_or_else(|| name.clone());
            NodeKind::Impl { for_struct, for_trait, generics, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_: false }
        }
        DefKind::Fn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let async_ = tcx.asyncness(def_id).is_async();
            let ret_raw = fmt_ty(tcx, sig.output().skip_binder());
            let ret = if async_ { unwrap_future_output(&ret_raw) } else { ret_raw };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let body = hir_body_src(tcx, def_id);
            NodeKind::Function { name, vis, generics, params, ret, body, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        }
        DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            let params = map_params(tcx, def_id, sig.inputs().skip_binder());
            let async_ = tcx.asyncness(def_id).is_async();
            let ret_raw = fmt_ty(tcx, sig.output().skip_binder());
            let ret = if async_ { unwrap_future_output(&ret_raw) } else { ret_raw };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let body = hir_body_src(tcx, def_id);
            NodeKind::Method { name, vis, generics, params, ret, body, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
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
        // Filter out synthetic impl-trait params (named "impl Trait").
        .filter(|p| !p.name.as_str().starts_with("impl "))
        .map(|p| {
            let name = p.name.to_string();
            let bounds = bounds_map
                .remove(&name)
                .unwrap_or_default()
                .into_iter()
                .map(|b| normalize_bound(&b))
                .filter(|b| b != "Sized" && b != "MetaSized")
                .collect();
            let is_lifetime = matches!(p.kind, ty::GenericParamDefKind::Lifetime);
            GenericParam { name, bounds, is_lifetime, default_ty: None }
        })
        .collect()
}

/// Strip stdlib path prefixes from a trait bound string.
/// "std::marker::Sized" -> "Sized", "std::cmp::PartialOrd" -> "PartialOrd", etc.
fn normalize_bound(b: &str) -> String {
    const BOUND_PREFIXES: &[&str] = &[
        "std::marker::", "core::marker::", "std::cmp::", "core::cmp::",
        "std::fmt::", "core::fmt::", "std::clone::", "core::clone::",
        "std::ops::", "core::ops::", "std::convert::", "core::convert::",
        "std::iter::", "core::iter::", "std::future::", "core::future::",
        "std::hash::", "core::hash::",
    ];
    for prefix in BOUND_PREFIXES {
        if let Some(short) = b.strip_prefix(prefix) {
            return short.to_string();
        }
    }
    b.to_string()
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
                // Normalize self type: &User -> &Self, User -> Self.
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

/// For trait method declarations, HIR param names come from the TraitItem,
/// not from a body (which doesn't exist for declarations). Fall back to
/// walking the HIR TraitItem's fn decl for ident names.
fn map_trait_method_params(tcx: TyCtxt<'_>, def_id: DefId, inputs: &[ty::Ty<'_>]) -> Vec<Param> {
    // Try to get param idents from HIR TraitItem fn decl.
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
    // Get self status from AssocItem.
    let has_self = inputs.first().map_or(false, |ty| {
        // The first input is `&Self` or `Self` for methods with self.
        // Check via ty string — crude but reliable for our purposes.
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
                hir_names
                    .get(i)
                    .and_then(|n| n.clone())
                    .unwrap_or_else(|| format!("p{i}"))
            };

            let ty_str = if is_self {
                let raw = fmt_ty(tcx, *ty);
                if raw.starts_with('&') {
                    "&Self".to_string()
                } else {
                    "Self".to_string()
                }
            } else {
                fmt_ty(tcx, *ty)
            };

            Param { name, ty: ty_str, is_self, mutable: false, lifetime: None }
        })
        .collect()
}

/// Unwrap `impl Future<Output = T>` or `impl Future<Output=T>` → `T`.
/// Used to normalize async fn return types so the emit layer can render
/// `async fn foo() -> T` instead of `async fn foo() -> impl Future<Output=T>`.
fn unwrap_future_output(ret: &str) -> String {
    // Matches: "impl Future<Output = T>" or "impl Future<Output=T>"
    let trimmed = ret.trim();
    if let Some(inner) = trimmed.strip_prefix("impl Future<Output") {
        let inner = inner.trim_start_matches(|c: char| c.is_whitespace());
        if let Some(inner) = inner.strip_prefix("=") {
            let inner = inner.trim();
            // Strip trailing '>'
            if let Some(t) = inner.strip_suffix('>') {
                return t.trim().to_string();
            }
        }
    }
    ret.to_string()
}

/// Slice the source text of a Const or Static initializer expression.
/// Falls back to empty string if the item has no local body or the span
/// is synthetic (macros, autogenerated code).
fn hir_init_src(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let Some(local) = def_id.as_local() else { return String::new() };
    // Nightly-stable safe body extraction
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
            // For async fns, fn_sig output is `impl Future<Output = T>`.
            // Unwrap to T so emit produces `async fn ... -> T`.
            let ret = if tcx.asyncness(item.def_id).is_async() { unwrap_future_output(&ret) } else { ret };
            let unsafe_ = sig.safety() == Safety::Unsafe;
            let async_ = tcx.asyncness(item.def_id).is_async();
            let generics = map_generics(tcx, item.def_id);
            let vis = map_vis(tcx.visibility(item.def_id));
            TraitMethod { name: item.name().to_string(), vis, generics, params, ret, body: Body::None, attrs: Vec::new(), where_clauses: Vec::new(), unsafe_, async_ }
        })
        .collect()
}

/// Capture function/method body as Body::Raw(source) via HIR body span.
/// Returns Body::None for trait declarations and extern fns (no local body).
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

/// Strip a single layer of outer `{ ... }` braces from a body snippet,
/// trimming whitespace. The HIR body value span includes the block braces,
/// but ModelIR Body::Raw stores just the inner content.
fn strip_outer_braces(src: String) -> String {
    let trimmed = src.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        // Remove first '{' and last '}', then trim inner whitespace.
        let inner = &trimmed[1..trimmed.len() - 1];
        // Dedent: find minimum indentation and strip it.
        let lines: Vec<&str> = inner.lines().collect();
        // Find minimum leading whitespace among non-empty lines.
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
