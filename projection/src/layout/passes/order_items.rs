use super::{LayoutCtx, LayoutPass};
use crate::layout::{FilePlan, ImplPlan, ItemPlan, ModuleDeclPlan, Plan};
use model::ir::node::NodeKind;

pub struct OrderItems;

impl LayoutPass for OrderItems {
    fn run(&self, plan: &mut Plan, _ctx: &LayoutCtx) {
        for file in &mut plan.files {
            sort_items(&mut file.items);
        }
    }
}

fn sort_items(items: &mut Vec<ItemPlan>) {
    for item in items.iter_mut() {
        match item {
            ItemPlan::Module(ModuleDeclPlan { items: child, .. }) => sort_items(child),
            ItemPlan::Impl(ImplPlan { .. }) => {}
            _ => {}
        }
    }
    items.sort_by(|a, b| item_key(a).cmp(&item_key(b)));
}

fn item_key(item: &ItemPlan) -> (u8, String) {
    match item {
        ItemPlan::Leaf(k) => (kind_priority(k), kind_name(k)),
        ItemPlan::Impl(ImplPlan { for_struct, for_trait, .. }) => (8, format!("{}::{:?}", for_struct, for_trait)),
        ItemPlan::Module(ModuleDeclPlan { name, .. }) => (200, name.clone()),
        ItemPlan::CargoToml { .. } => (250, String::from("cargo")),
    }
}

pub fn kind_priority(k: &NodeKind) -> u8 {
    match k {
        NodeKind::ExternCrate { .. } => 0,
        NodeKind::Use { .. } => 1,
        NodeKind::TypeAlias { .. } => 2,
        NodeKind::Const { .. } => 3,
        NodeKind::Static { .. } => 4,
        NodeKind::Struct { .. } => 5,
        NodeKind::Enum { .. } => 6,
        NodeKind::Trait { .. } => 7,
        NodeKind::Impl { .. } => 8,
        NodeKind::Function { .. } => 9,
        _ => 255,
    }
}

fn kind_name(k: &NodeKind) -> String {
    match k {
        NodeKind::Struct { name, .. }
        | NodeKind::Enum { name, .. }
        | NodeKind::Trait { name, .. }
        | NodeKind::Impl { for_struct: name, .. }
        | NodeKind::Function { name, .. }
        | NodeKind::Method { name, .. }
        | NodeKind::TypeAlias { name, .. }
        | NodeKind::Const { name, .. }
        | NodeKind::Static { name, .. }
        | NodeKind::TypeRef { name, .. }
        | NodeKind::ExternCrate { name, .. } => name.clone(),
        NodeKind::Use { path, .. } => path.clone(),
        NodeKind::MacroCall { path, .. } => path.clone(),
        NodeKind::Crate { name, .. } => name.clone(),
        NodeKind::Module { path, .. } => path.clone(),
        NodeKind::Lifetime { name, .. } => name.clone(),
    }
}
