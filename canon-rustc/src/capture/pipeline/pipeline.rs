use anyhow::Result;
use canon_ir::ir::CanonIR;
use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use crate::capture::pipeline::{body, engine, relations};
use crate::capture::{assembler, index, Partial};
use crate::log::append_panic_record;
use std::any::Any;
use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn project_def(tcx: TyCtxt<'_>, def_id: DefId, index: &crate::capture::index::Index) -> Partial {
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

    let mut partials = Vec::with_capacity(idx.def_ids.len());
    let mut seen: HashSet<DefId> = HashSet::with_capacity(idx.def_ids.len());
    for def_id in idx.def_ids.iter().copied() {
        if !seen.insert(def_id) {
            continue;
        }
        let result = catch_unwind(AssertUnwindSafe(|| project_def(tcx, def_id, &idx)));
        match result {
            Ok(partial) => partials.push(partial),
            Err(payload) => {
                let def_path = tcx.def_path_str(def_id);
                let message = panic_message(&payload);
                append_panic_record(&def_path, &message);
                partials.push(Partial {
                    panic_def_id: Some(def_path),
                    ..Partial::default()
                });
            }
        }
    }

    let canon = assembler::canon_assemble(tcx, &idx, partials).map_err(|e| {
        let msg = format!("{e:#}");
        append_panic_record("canon_assemble", &msg);
        anyhow::anyhow!("canon_assemble failed: {msg}")
    })?;
    if let Err(err) = super::validate::structural::validate(&canon) {
        append_panic_record("kernel_structural_violation", &format!("{err:?}"));
        return Err(err);
    }
    Ok(canon)
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "unknown panic payload".to_string()
}
