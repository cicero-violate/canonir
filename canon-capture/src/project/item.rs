use crate::types::{Body, EdgeHint, Node};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::index::Index;
use crate::capture::engine;

/// Structural projection: DefId -> NodeKind using rule engine.
/// Legacy path is retained as compatibility shim.
pub fn project_item(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> (Vec<Node>, Vec<EdgeHint>) {
    if let Some(out) = engine::lower_def(tcx, def_id, index) {
        return out;
    }
    project_item_legacy(tcx, def_id, index)
}

pub(crate) fn project_item_legacy(tcx: TyCtxt<'_>, def_id: DefId, index: &Index) -> (Vec<Node>, Vec<EdgeHint>) {
    let _ = tcx;
    let _ = def_id;
    let _ = index;
    (Vec::new(), Vec::new())
}

pub(crate) fn mir_body_structural(tcx: TyCtxt<'_>, def_id: DefId, param_names: &[String], returns_unit: bool) -> Body {
    crate::capture::mir::lower::mir_body_structural(tcx, def_id, param_names, returns_unit)
}

pub(crate) fn mir_body_structural_legacy(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    param_names: &[String],
    returns_unit: bool,
) -> Body {
    crate::capture::mir::lower::mir_body_structural(tcx, def_id, param_names, returns_unit)
}
