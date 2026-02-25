use model::ir::node::{NodeKind, Visibility};

use super::{LayoutCtx, LayoutPass};
use crate::layout::{ImplPlan, ItemPlan, ModuleDeclPlan, Plan};

pub struct NormalizeVisibility;

impl LayoutPass for NormalizeVisibility {
    fn run(&self, plan: &mut Plan, _ctx: &LayoutCtx) {
        for file in &mut plan.files {
            normalize_items(&mut file.items);
        }
    }
}

fn normalize_items(items: &mut [ItemPlan]) {
    for item in items.iter_mut() {
        match item {
            ItemPlan::Impl(ImplPlan { for_trait: Some(_), methods, .. }) => {
                for m in methods {
                    if let NodeKind::Method { vis, .. } = m {
                        *vis = Visibility::Private;
                    }
                }
            }
            ItemPlan::Module(ModuleDeclPlan { items: child, .. }) => normalize_items(child),
            _ => {}
        }
    }
}
