use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::index::Index;
use crate::norm;
use crate::types::{EdgeHint, EdgeKind, EnumVariant, Node, NodeId, NodeKind, StructKind, Visibility};

use super::item;
use super::rules::{DefMeta, RuleSpec, RULES};

/// Analyze one definition into reusable metadata for rule matching.
pub fn analyze_def(tcx: TyCtxt<'_>, def_id: DefId) -> DefMeta {
    let def_kind = tcx.def_kind(def_id);
    let has_body = tcx.hir_maybe_body_owned_by(def_id.expect_local()).is_some();
    let is_trait_item = matches!(def_kind, DefKind::AssocFn | DefKind::AssocConst | DefKind::AssocTy)
        && tcx
            .opt_parent(def_id)
            .is_some_and(|p| matches!(tcx.def_kind(p), DefKind::Trait));
    let is_assoc_item = matches!(def_kind, DefKind::AssocFn | DefKind::AssocConst | DefKind::AssocTy);

    DefMeta {
        def_id,
        def_kind,
        has_body,
        is_trait_item,
        is_assoc_item,
    }
}

/// Select the first matching rule (ordered table semantics).
pub fn select_rule<'a>(meta: &DefMeta, rules: &'a [RuleSpec]) -> Option<&'a RuleSpec> {
    rules.iter().find(|r| r.pred.matches(meta))
}

/// Engine entrypoint scaffold.
/// Returns `None` in Phase 1 so legacy lowering remains authoritative.
pub fn lower_def(_tcx: TyCtxt<'_>, def_id: DefId, _index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let meta = analyze_def(_tcx, def_id);
    let rule = select_rule(&meta, RULES)?;
    match &rule.emit {
        super::rules::RuleEmit::Hook("legacy_passthrough") => Some(item::project_item_legacy(_tcx, def_id, _index)),
        super::rules::RuleEmit::Template("mod_item") => lower_mod_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("struct_like_item") => lower_struct_like_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("enum_item") => lower_enum_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("fn_item") => lower_fn_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("assoc_fn_item") => lower_assoc_fn_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("const_item") => lower_const_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("static_item") => lower_static_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("ty_alias_item") => lower_ty_alias_item(_tcx, def_id, _index),
        super::rules::RuleEmit::Template("use_item") => lower_use_item(_tcx, def_id, _index),
        _ => None,
    }
}

fn lower_mod_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let raw_span = tcx.def_span(def_id);
    let span = Some(norm::span(tcx, raw_span));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let file = norm::module_file(tcx, def_id);
    let decl_file = norm::file(tcx, raw_span);
    let inline = file == decl_file
        && def_id.as_local().is_some_and(|local| {
            if let rustc_hir::Node::Item(item_hir) = tcx.hir_node_by_def_id(local) {
                matches!(item_hir.kind, rustc_hir::ItemKind::Mod(_, _))
            } else {
                false
            }
        });
    let kind = NodeKind::Module {
        path: norm::module_path(tcx, def_id),
        file,
        vis,
        inline,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_struct_like_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let (generics, where_clauses) = item::map_generics(tcx, def_id);
    let adt = tcx.adt_def(def_id);
    let variant = adt.non_enum_variant();
    let struct_kind = match variant.ctor_kind() {
        Some(rustc_hir::def::CtorKind::Fn) => StructKind::Tuple,
        Some(rustc_hir::def::CtorKind::Const) => StructKind::Unit,
        _ => StructKind::Named,
    };
    let fields = item::map_fields(tcx, variant.fields.iter(), false);
    let derives = item::collect_derives(tcx, def_id);
    let kind = NodeKind::Struct {
        name,
        vis,
        generics,
        fields,
        derives,
        attrs: Vec::new(),
        where_clauses,
        struct_kind,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_enum_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let (generics, where_clauses) = item::map_generics(tcx, def_id);
    let adt = tcx.adt_def(def_id);
    let variants: Vec<EnumVariant> = adt
        .variants()
        .iter()
        .map(|v| EnumVariant {
            name: v.name.to_string(),
            fields: item::map_fields(tcx, v.fields.iter(), true),
        })
        .collect();
    let derives = item::collect_derives(tcx, def_id);
    let kind = NodeKind::Enum {
        name,
        vis,
        generics,
        variants,
        derives,
        attrs: Vec::new(),
        where_clauses,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_fn_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let (generics, where_clauses) = item::map_generics(tcx, def_id);
    let sig = tcx.fn_sig(def_id).skip_binder();
    let returns_unit = sig.output().skip_binder().is_unit();
    let params = item::map_params(tcx, def_id, sig.inputs().skip_binder());
    let async_ = tcx.asyncness(def_id).is_async();
    let ret = item::declared_fn_return_type_expr(tcx, def_id)
        .unwrap_or_else(|| item::lower_ty(tcx, sig.output().skip_binder()));
    let unsafe_ = sig.safety() == rustc_hir::Safety::Unsafe;
    let param_names = params.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    let body = item::mir_body_structural(tcx, def_id, &param_names, returns_unit);
    let kind = NodeKind::Function {
        name,
        vis,
        generics,
        params,
        ret,
        body,
        attrs: Vec::new(),
        where_clauses,
        unsafe_,
        async_,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_assoc_fn_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let (generics, where_clauses) = item::map_generics(tcx, def_id);
    let sig = tcx.fn_sig(def_id).skip_binder();
    let returns_unit = sig.output().skip_binder().is_unit();
    let params = item::map_params(tcx, def_id, sig.inputs().skip_binder());
    let async_ = tcx.asyncness(def_id).is_async();
    let ret = item::declared_fn_return_type_expr(tcx, def_id)
        .unwrap_or_else(|| item::lower_ty(tcx, sig.output().skip_binder()));
    let unsafe_ = sig.safety() == rustc_hir::Safety::Unsafe;
    let param_names = params.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    let body = item::mir_body_structural(tcx, def_id, &param_names, returns_unit);
    let kind = NodeKind::Method {
        name,
        vis,
        generics,
        params,
        ret,
        body,
        attrs: Vec::new(),
        where_clauses,
        unsafe_,
        async_,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_const_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let ty_expr = item::declared_item_ty_expr(tcx, def_id)
        .unwrap_or_else(|| item::lower_ty(tcx, tcx.type_of(def_id).instantiate_identity()));
    let value = item::hir_init_src(tcx, def_id);
    let kind = NodeKind::Const {
        name,
        vis,
        ty: ty_expr,
        value,
        attrs: Vec::new(),
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_ty_alias_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let (generics, where_clauses) = item::map_generics(tcx, def_id);
    let ty_expr = item::declared_item_ty_expr(tcx, def_id)
        .unwrap_or_else(|| item::lower_ty(tcx, tcx.type_of(def_id).instantiate_identity()));
    let kind = NodeKind::TypeAlias {
        name,
        vis,
        generics,
        ty: ty_expr,
        attrs: Vec::new(),
        where_clauses,
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_static_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let full_path = norm::path(tcx, def_id);
    let name = norm::short(&full_path).to_string();
    let span = Some(norm::span(tcx, tcx.def_span(def_id)));
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let ty_expr = item::declared_item_ty_expr(tcx, def_id)
        .unwrap_or_else(|| item::lower_ty(tcx, tcx.type_of(def_id).instantiate_identity()));
    let value = item::hir_init_src(tcx, def_id);
    let mutable = matches!(tcx.def_kind(def_id), DefKind::Static { mutability: rustc_hir::Mutability::Mut, .. });
    let kind = NodeKind::Static {
        name,
        vis,
        ty: ty_expr,
        value,
        mutable,
        attrs: Vec::new(),
    };
    Some((vec![Node { id, kind, span }], vec![]))
}

fn lower_use_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Option<(Vec<Node>, Vec<EdgeHint>)> {
    let id = *index.def_to_node.get(&def_id)?;
    let local = def_id.as_local()?;
    let rustc_hir::Node::Item(item_hir) = tcx.hir_node_by_def_id(local) else {
        return Some((Vec::new(), Vec::new()));
    };
    let rustc_hir::ItemKind::Use(use_path, use_kind) = item_hir.kind else {
        return Some((Vec::new(), Vec::new()));
    };
    let raw_span = tcx.def_span(def_id);
    let span_str = norm::span(tcx, raw_span);
    let vis = item::map_vis(tcx, def_id, tcx.visibility(def_id));
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<EdgeHint> = Vec::new();
    let glob = matches!(use_kind, rustc_hir::UseKind::Glob);
    let parent_src = tcx
        .opt_parent(def_id)
        .and_then(|p| index.def_to_node.get(&p))
        .map(|nid| nid.index() as u32);

    for (ordinal, res) in use_path.res.iter().flatten().enumerate() {
        let rustc_hir::def::Res::Def(_, target_did) = res else {
            continue;
        };
        let path = norm::path(tcx, *target_did);
        if path.is_empty() {
            continue;
        }
        let node_id = if ordinal == 0 {
            id
        } else {
            synthetic_use_id(id, ordinal as u32)
        };
        let alias = match use_kind {
            rustc_hir::UseKind::Single(ident) if ordinal == 0 => {
                let leaf = path.rsplit("::").next().unwrap_or("");
                if ident.name.as_str() != leaf {
                    Some(ident.to_string())
                } else {
                    None
                }
            }
            _ => None,
        };
        nodes.push(Node {
            id: node_id,
            kind: NodeKind::Use {
                vis: vis.clone(),
                path,
                alias,
                glob,
            },
            span: Some(span_str.clone()),
        });

        if let Some(parent) = parent_src {
            edges.push(EdgeHint {
                src: parent,
                dst: node_id.index() as u32,
                kind: EdgeKind::Contains,
            });
        }
        if let Some(&target_node) = index.def_to_node.get(target_did) {
            edges.push(EdgeHint {
                src: node_id.index() as u32,
                dst: target_node.index() as u32,
                kind: EdgeKind::Resolves,
            });
            if is_public_vis(&vis) {
                edges.push(EdgeHint {
                    src: node_id.index() as u32,
                    dst: target_node.index() as u32,
                    kind: EdgeKind::Reexports,
                });
            }
        }
    }

    Some((nodes, edges))
}

fn synthetic_use_id(base: NodeId, ordinal: u32) -> NodeId {
    NodeId(1_000_000_000u32 + base.0.saturating_mul(1024) + ordinal)
}

fn is_public_vis(v: &Visibility) -> bool {
    !matches!(v, Visibility::Private)
}
