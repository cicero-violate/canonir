use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::{index::Index, Partial};

pub mod body;
pub mod item;
pub mod relations;

/// Project a single definition into a Partial (nodes + local edge hints).
pub fn project_def(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> Partial {
    let mut partial = Partial::default();

    // Structural node emission.
    if let Some(node) = item::project_item(tcx, def_id, index) {
        partial.nodes.push(node);
    }

    // Relations directly derivable from the item (parent/module/impl/trait edges).
    partial.edge_hints.extend(relations::project_relations(tcx, def_id, index));

    // MIR / bodies for functions (calls, cfg, const deps, outlives edges).
    partial.edge_hints.extend(body::project_body(tcx, def_id, index));

    partial
}
