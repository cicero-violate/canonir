use rustc_driver::run_compiler;

use crate::hir::HirCollector;
use crate::mir::MirCollector;
use model::model_ir::Model;

pub fn run_hir(args: &[String]) -> Model {
    let mut callbacks = HirCollector::new();
    run_compiler(args, &mut callbacks);
    callbacks.model
}

pub fn run_mir(args: &[String]) -> Model {
    let mut callbacks = MirCollector::new();
    run_compiler(args, &mut callbacks);
    callbacks.model
}
