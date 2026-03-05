use std::collections::HashMap;

use super::module_normalization::{module_to_fs_path, ModulePath};

pub type ItemId = String;

#[derive(Debug, Clone)]
pub struct EmitEntry {
    pub item: ItemId,
    pub module: ModulePath,
    pub path: String,
}

pub fn compute_emit_plan(crate_name: &str, ordered_items: &[(ItemId, ModulePath)]) -> Vec<EmitEntry> {
    let mut plan = Vec::new();

    for (item, module) in ordered_items {
        let path = module_to_fs_path(crate_name, module);

        plan.push(EmitEntry { item: item.clone(), module: module.clone(), path });
    }

    plan
}
