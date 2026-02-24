use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::DefKind;
use rustc_interface::interface::Compiler;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;

use model::model_ir::{MirBasicBlock, MirBody, Model};

pub struct MirCollector {
    pub model: Model,
}

impl MirCollector {
    pub fn new() -> Self {
        Self { model: Model::new() }
    }
}

impl Callbacks for MirCollector {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        let crate_items = tcx.hir_crate_items(());

        for local_def_id in crate_items.definitions() {
            let def_id: DefId = local_def_id.to_def_id();
            let def_path = tcx.def_path_str(def_id);

            match tcx.def_kind(def_id) {
                DefKind::Fn | DefKind::AssocFn => {
                    // Skip items without bodies (e.g. trait method declarations)
                    if tcx.hir_maybe_body_owned_by(local_def_id).is_none() {
                        continue;
                    }

                    // Skip functions without bodies (e.g. trait method declarations)
                    if tcx.is_const_fn(def_id) || tcx.is_constructor(def_id) {
                        // allow normal fns; these helpers just avoid weird edge cases
                    }

                    // Guard MIR query to avoid ICE propagation
                    let body = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tcx.optimized_mir(def_id))) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };

                    let mut blocks = Vec::new();

                    for (bb_idx, bb) in body.basic_blocks.iter_enumerated() {
                        let statements = bb.statements.iter().map(|s| format!("{:?}", s)).collect();

                        let terminator = Some(format!("{:?}", bb.terminator()));

                        blocks.push(MirBasicBlock { index: bb_idx.index(), statements, terminator });
                    }

                    self.model.mir_bodies.push(MirBody { def_id: def_path.clone(), basic_blocks: blocks });
                }
                _ => {}
            }
        }

        Compilation::Continue
    }
}
