use crate::types::{EdgeHint, EdgeKind};
use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::index::Index;

/// Relations derivable from item metadata (module parent, impl target, etc.).
pub fn project_relations(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Vec<EdgeHint> {
    let Some(&id) = index.def_to_node.get(&def_id) else {
        return Vec::new();
    };
    let mut edges = Vec::new();

    // ── Parent containment: parent --[Contains]--> item ──────────────────────
    if let Some(parent) = tcx.opt_parent(def_id) {
        if let Some(&pid) = index.def_to_node.get(&parent) {
            edges.push(EdgeHint { src: pid.index() as u32, dst: id.index() as u32, kind: EdgeKind::Contains });
        }
    }

    // ── Impl relationships ───────────────────────────────────────────────────
    if matches!(tcx.def_kind(def_id), DefKind::Impl { .. }) {
        // ImplFor: impl --[ImplFor]--> struct/type being implemented
        // type_of on an impl DefId gives the self type (the struct).
        let self_ty = tcx.type_of(def_id).instantiate_identity();
        if let Some(adt_def_id) = self_ty.ty_adt_def().map(|adt| adt.did()) {
            if let Some(&struct_node) = index.def_to_node.get(&adt_def_id) {
                edges.push(EdgeHint { src: id.index() as u32, dst: struct_node.index() as u32, kind: EdgeKind::ImplFor });
            }
        }

        // Resolves: impl --[Resolves]--> trait (only for trait impls, not inherent)
        if let Some(&trait_node) = tcx.impl_opt_trait_ref(def_id).and_then(|eb| index.def_to_node.get(&eb.skip_binder().def_id)) {
            edges.push(EdgeHint { src: id.index() as u32, dst: trait_node.index() as u32, kind: EdgeKind::Resolves });
        }
    }

    edges
}
