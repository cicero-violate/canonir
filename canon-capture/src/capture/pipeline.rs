use anyhow::Result;
use canon::ir::CanonIR;
use rustc_middle::ty::TyCtxt;

use crate::{canon_assemble, index, project, Partial};

pub fn capture(tcx: TyCtxt<'_>) -> Result<CanonIR> {
    let idx = index::build_index(tcx);

    let partials: Vec<Partial> = idx
        .def_ids
        .iter()
        .map(|d| project::project_def(tcx, *d, &idx))
        .collect();

    let canon = canon_assemble::canon_assemble(tcx, &idx, partials);
    super::validate::structural::validate(&canon)?;
    Ok(canon)
}
