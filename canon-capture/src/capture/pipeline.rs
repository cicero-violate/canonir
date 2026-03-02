use anyhow::Result;
use canon::ir::CanonIR;
use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::capture::{body, engine, relations};
use crate::{canon_assemble, index, Partial};

fn project_def(tcx: TyCtxt<'_>, def_id: DefId, index: &crate::index::Index) -> Partial {
    let mut partial = Partial::default();

    if let Some((nodes, item_edges)) = engine::lower_def(tcx, def_id, index) {
        partial.nodes.extend(nodes);
        partial.edge_hints.extend(item_edges);
    }

    if !matches!(tcx.def_kind(def_id), DefKind::Use) {
        partial.edge_hints.extend(relations::project_relations(tcx, def_id, index));
    }

    let (body_nodes, body_edges) = body::project_body(tcx, def_id, index);
    partial.nodes.extend(body_nodes);
    partial.edge_hints.extend(body_edges);

    partial
}

pub fn capture(tcx: TyCtxt<'_>) -> Result<CanonIR> {
    let idx = index::build_index(tcx);

    let partials: Vec<Partial> = idx.def_ids.iter().map(|d| project_def(tcx, *d, &idx)).collect();

    let canon = canon_assemble::canon_assemble(tcx, &idx, partials);
    super::validate::structural::validate(&canon)?;
    Ok(canon)
}
