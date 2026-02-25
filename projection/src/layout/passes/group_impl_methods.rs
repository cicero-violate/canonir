use model::ir::edge::EdgeKind;
use model::ir::node::NodeKind;

use super::{LayoutCtx, LayoutPass};
use crate::layout::{ItemPlan, Plan};

pub struct GroupImplMethods;

impl LayoutPass for GroupImplMethods {
    fn run(&self, plan: &mut Plan, ctx: &LayoutCtx) {
        for file in &mut plan.files {
            group_in_items(&mut file.items, ctx);
        }
    }
}

fn group_in_items(items: &mut [ItemPlan], ctx: &LayoutCtx) {
    for item in items.iter_mut() {
        match item {
            ItemPlan::Impl(impl_plan) => {
                if let Some(id) = impl_plan.node_id {
                    let mut methods = Vec::new();
                    for (child_id, edge) in ctx.ir.module_graph.neighbours(id) {
                        if *edge != EdgeKind::Contains {
                            continue;
                        }
                        let child = ctx.ir.node(child_id);
                        if matches!(child.kind, NodeKind::Method { .. }) {
                            methods.push(child.kind.clone());
                        }
                    }
                    impl_plan.methods = methods;
                }
            }
            ItemPlan::Module(m) => {
                group_in_items(&mut m.items, ctx);
            }
            _ => {}
        }
    }
}
