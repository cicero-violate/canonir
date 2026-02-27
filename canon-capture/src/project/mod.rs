use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::{index::Index, Partial};

pub mod body;
pub mod engine;
pub mod item;
pub mod relations;
pub mod rules;

/// Project a single definition into a Partial (nodes + local edge hints).
pub fn project_def(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Partial {
    let mut partial = Partial::default();

    // Structural node emission.
    let (nodes, item_edges) = item::project_item(tcx, def_id, index);
    partial.nodes.extend(nodes);
    partial.edge_hints.extend(item_edges);

    // Relations directly derivable from the item (parent/module/impl/trait edges).
    if !matches!(tcx.def_kind(def_id), rustc_hir::def::DefKind::Use) {
        partial.edge_hints.extend(relations::project_relations(tcx, def_id, index));
    }

    // MIR / bodies for functions (calls, cfg, const deps, structural PathRef).
    let (body_nodes, body_edges) = body::project_body(tcx, def_id, index);
    partial.nodes.extend(body_nodes);
    partial.edge_hints.extend(body_edges);

    partial
}
