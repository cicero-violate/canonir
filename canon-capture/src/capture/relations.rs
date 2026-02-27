use crate::types::EdgeHint;
use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::capture::edge_emit;
use crate::index::Index;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelationTemplate {
    ParentContains,
    ParentAssocItem,
    ImplFor,
    ImplRef,
}

fn relation_templates(def_kind: DefKind) -> &'static [RelationTemplate] {
    match def_kind {
        DefKind::Impl { .. } => &[
            RelationTemplate::ParentContains,
            RelationTemplate::ImplFor,
            RelationTemplate::ImplRef,
        ],
        DefKind::AssocFn | DefKind::AssocTy | DefKind::AssocConst => &[
            RelationTemplate::ParentContains,
            RelationTemplate::ParentAssocItem,
        ],
        _ => &[RelationTemplate::ParentContains],
    }
}

fn push_parent_contains(
    edges: &mut Vec<EdgeHint>,
    tcx: TyCtxt<'_>,
    def_id: DefId,
    id_u32: u32,
    index: &Index,
) -> Option<u32> {
    let parent = tcx.opt_parent(def_id)?;
    let pid = *index.def_to_node.get(&parent)?;
    let parent_u32 = pid.index() as u32;
    edge_emit::push_contains(edges, parent_u32, id_u32);
    Some(parent_u32)
}

fn maybe_push_parent_assoc_item(
    edges: &mut Vec<EdgeHint>,
    tcx: TyCtxt<'_>,
    def_id: DefId,
    parent_u32: Option<u32>,
    id_u32: u32,
) {
    let Some(parent) = tcx.opt_parent(def_id) else {
        return;
    };
    if !matches!(
        tcx.def_kind(def_id),
        DefKind::AssocFn | DefKind::AssocTy | DefKind::AssocConst
    ) {
        return;
    }
    if !matches!(tcx.def_kind(parent), DefKind::Trait | DefKind::Impl { .. }) {
        return;
    }
    let Some(src) = parent_u32 else {
        return;
    };
    edge_emit::push_assoc_item(edges, src, id_u32);
}

fn maybe_push_impl_for(
    edges: &mut Vec<EdgeHint>,
    tcx: TyCtxt<'_>,
    def_id: DefId,
    id_u32: u32,
    index: &Index,
) {
    let self_ty = tcx.type_of(def_id).instantiate_identity();
    let Some(adt_def_id) = self_ty.ty_adt_def().map(|adt| adt.did()) else {
        return;
    };
    let Some(&struct_node) = index.def_to_node.get(&adt_def_id) else {
        return;
    };
    edge_emit::push_impl_for(edges, id_u32, struct_node.index() as u32);
}

fn maybe_push_impl_ref(
    edges: &mut Vec<EdgeHint>,
    tcx: TyCtxt<'_>,
    def_id: DefId,
    id_u32: u32,
    index: &Index,
) {
    let Some(&trait_node) = tcx
        .impl_opt_trait_ref(def_id)
        .and_then(|eb| index.def_to_node.get(&eb.skip_binder().def_id))
    else {
        return;
    };
    edge_emit::push_impl_ref(edges, id_u32, trait_node.index() as u32);
}

pub fn project_relations(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Vec<EdgeHint> {
    let Some(&id) = index.def_to_node.get(&def_id) else {
        return Vec::new();
    };
    let mut edges = Vec::new();
    let id_u32 = id.index() as u32;
    let templates = relation_templates(tcx.def_kind(def_id));
    let mut parent_u32: Option<u32> = None;

    for template in templates {
        match template {
            RelationTemplate::ParentContains => {
                parent_u32 = push_parent_contains(&mut edges, tcx, def_id, id_u32, index);
            }
            RelationTemplate::ParentAssocItem => {
                maybe_push_parent_assoc_item(&mut edges, tcx, def_id, parent_u32, id_u32);
            }
            RelationTemplate::ImplFor => {
                maybe_push_impl_for(&mut edges, tcx, def_id, id_u32, index);
            }
            RelationTemplate::ImplRef => {
                maybe_push_impl_ref(&mut edges, tcx, def_id, id_u32, index);
            }
        }
    }

    edges
}
