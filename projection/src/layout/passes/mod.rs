use model::ir::{model_ir::ModelIR, node::NodeKind};

use crate::layout::Plan;

mod group_impl_methods;
mod inject_imports;
mod normalize_visibility;
mod sanitize_generics;
mod order_items;

pub trait LayoutPass {
    fn run(&self, plan: &mut Plan, ctx: &LayoutCtx);
}

pub struct LayoutCtx<'a> {
    pub ir: &'a ModelIR,
    pub defined_types: std::collections::HashSet<String>,
}

impl<'a> LayoutCtx<'a> {
    pub fn new(ir: &'a ModelIR) -> Self {
        let mut defined = std::collections::HashSet::new();
        for n in &ir.nodes {
            match &n.kind {
                NodeKind::Struct { name, .. } | NodeKind::Enum { name, .. } | NodeKind::Trait { name, .. } | NodeKind::TypeAlias { name, .. } => {
                    defined.insert(name.clone());
                }
                _ => {}
            }
        }
        LayoutCtx { ir, defined_types: defined }
    }
}

pub fn run_layout_passes(plan: &mut Plan, ctx: &LayoutCtx) {
    let passes: Vec<Box<dyn LayoutPass>> = vec![
        Box::new(group_impl_methods::GroupImplMethods),
        Box::new(sanitize_generics::SanitizeGenerics),
        Box::new(normalize_visibility::NormalizeVisibility),
        Box::new(inject_imports::InjectImports),
        Box::new(order_items::OrderItems),
    ];

    for p in passes {
        p.run(plan, ctx);
    }
}
