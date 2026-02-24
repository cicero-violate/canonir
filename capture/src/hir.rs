use rustc_driver::{Callbacks, Compilation};
use rustc_hir::intravisit::{self, Visitor};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::TyCtxt;

use model::model_ir::{HirExpr, HirItem, Model};

pub struct HirCollector {
    pub model: Model,
}

impl HirCollector {
    pub fn new() -> Self {
        Self { model: Model::new() }
    }
}

impl Callbacks for HirCollector {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        let crate_items = tcx.hir_crate_items(());

        // --- Top-level items ---
        for local_def_id in crate_items.definitions() {
            let def_id = local_def_id.to_def_id();

            // Restrict to named, stable DefKinds only
            use rustc_hir::def::DefKind;
            let def_kind = tcx.def_kind(def_id);

            match def_kind {
                DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Trait | DefKind::Fn | DefKind::AssocFn | DefKind::Mod | DefKind::TyAlias | DefKind::Const | DefKind::Static { .. } => {}
                _ => continue,
            }

            let module_str = tcx.opt_parent(def_id).map(|p| tcx.def_path_str(p)).unwrap_or_else(|| "crate_root".to_string());

            // Avoid querying generics_of on unexpected node kinds (e.g. crate root)
            let generics = match def_kind {
                DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Trait | DefKind::TyAlias => {
                    tcx.generics_of(def_id).own_params.iter().map(|g| g.name.to_string()).collect()
                }
                _ => Vec::new(),
            };

            self.model.hir_items.push(HirItem {
                def_id: tcx.def_path_str(def_id),
                kind: format!("{:?}", def_kind),
                name: tcx.def_path_str(def_id),
                module: module_str,
                generics,
                span: format!("{:?}", tcx.def_span(def_id)),
            });
        }

        // --- Walk expressions via intravisit ---
        struct ExprVisitor<'a> {
            model: &'a mut Model,
        }

        impl<'v, 'a> Visitor<'v> for ExprVisitor<'a> {
            type Result = ();

            fn visit_expr(&mut self, expr: &'v rustc_hir::Expr<'v>) {
                self.model.hir_exprs.push(HirExpr { hir_id: format!("{:?}", expr.hir_id), kind: format!("{:?}", expr.kind), span: format!("{:?}", expr.span), ty: None });

                intravisit::walk_expr(self, expr);
            }
        }

        let mut expr_visitor = ExprVisitor { model: &mut self.model };

        for owner in crate_items.owners() {
            let local_def_id = owner.def_id;
            if let Some(body) = tcx.hir_maybe_body_owned_by(local_def_id) {
                intravisit::walk_body(&mut expr_visitor, body);
            }
        }

        Compilation::Continue
    }
}
