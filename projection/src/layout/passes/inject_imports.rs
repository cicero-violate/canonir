use model::ir::node::{NodeKind, Visibility};

use super::{LayoutCtx, LayoutPass};
use crate::layout::{ItemPlan, ModuleDeclPlan, Plan};

pub struct InjectImports;

impl LayoutPass for InjectImports {
    fn run(&self, plan: &mut Plan, _ctx: &LayoutCtx) {
        for file in &mut plan.files {
            inject_in_items(&mut file.items);
        }
    }
}

fn inject_in_items(items: &mut Vec<ItemPlan>) {
    let mut needs_describable = false;
    let mut has_describable_use = false;

    for item in items.iter() {
        match item {
            ItemPlan::Leaf(NodeKind::Use { path, .. }) if path == "crate::traits::Describable" => {
                has_describable_use = true;
            }
            ItemPlan::Leaf(kind) => {
                if contains_describable(kind) {
                    needs_describable = true;
                }
            }
            ItemPlan::Impl(imp) => {
                for m in &imp.methods {
                    if contains_describable(m) {
                        needs_describable = true;
                    }
                }
            }
            ItemPlan::Module(_) => {}
            _ => {}
        }
    }

    // recurse into modules
    for item in items.iter_mut() {
        if let ItemPlan::Module(ModuleDeclPlan { items: child, .. }) = item {
            inject_in_items(child);
        }
    }

    if needs_describable && !has_describable_use {
        items.insert(0, ItemPlan::Leaf(NodeKind::Use { vis: Visibility::Private, path: "crate::traits::Describable".into(), alias: None, glob: false }));
    }
}

fn contains_describable(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Function { params, ret, .. } | NodeKind::Method { params, ret, .. } => params.iter().any(|p| p.ty.contains("Describable")) || ret.contains("Describable"),
        _ => false,
    }
}
