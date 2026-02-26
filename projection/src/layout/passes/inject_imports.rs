use model::ir::node::{NodeKind, Visibility};

use super::{LayoutCtx, LayoutPass};
use crate::layout::{ItemPlan, ModuleDeclPlan, Plan};

pub struct InjectImports;

impl LayoutPass for InjectImports {
    fn run(&self, plan: &mut Plan, _ctx: &LayoutCtx) {
        for file in &mut plan.files {
            let file_path = file.path.to_string_lossy().to_string();
            inject_in_items(&mut file.items, &file_path);
        }
    }
}

fn inject_in_items(items: &mut Vec<ItemPlan>, file_path: &str) {
    let mut needs_describable = false;
    let mut has_describable_use = false;
    let mut needs_std_path = false;
    let mut has_std_path_use = false;
    let mut needs_symbol = false;
    let mut has_symbol_use = false;

    for item in items.iter() {
        match item {
            ItemPlan::Leaf(NodeKind::Use { path, .. }) if path.ends_with("::Describable") => {
                has_describable_use = true;
            }
            ItemPlan::Leaf(NodeKind::Use { path, .. }) if path == "std::path::Path" => {
                has_std_path_use = true;
            }
            ItemPlan::Leaf(NodeKind::Use { path, .. }) if path == "crate::symbol::Symbol" => {
                has_symbol_use = true;
            }
            ItemPlan::Leaf(kind) => {
                if contains_describable(kind) {
                    needs_describable = true;
                }
                if contains_path_use(kind) {
                    needs_std_path = true;
                }
                if contains_symbol_use(kind) {
                    needs_symbol = true;
                }
            }
            ItemPlan::Impl(imp) => {
                for m in &imp.methods {
                    if contains_describable(m) {
                        needs_describable = true;
                    }
                    if contains_path_use(m) {
                        needs_std_path = true;
                    }
                    if contains_symbol_use(m) {
                        needs_symbol = true;
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
            inject_in_items(child, file_path);
        }
    }

    if needs_describable && !has_describable_use {
        items.insert(0, ItemPlan::Leaf(NodeKind::Use { vis: Visibility::Private, path: "crate::traits::Describable".into(), alias: None, glob: false }));
    }
    if needs_std_path && !has_std_path_use {
        items.insert(0, ItemPlan::Leaf(NodeKind::Use { vis: Visibility::Private, path: "std::path::Path".into(), alias: None, glob: false }));
    }
    if file_path != "src/symbol.rs" && needs_symbol && !has_symbol_use {
        items.insert(0, ItemPlan::Leaf(NodeKind::Use { vis: Visibility::Private, path: "crate::symbol::Symbol".into(), alias: None, glob: false }));
    }

    dedup_uses(items);
}

fn contains_describable(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Function { params, ret, .. } | NodeKind::Method { params, ret, .. } => params.iter().any(|p| p.ty.contains("Describable")) || ret.contains("Describable"),
        _ => false,
    }
}

fn contains_path_use(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Function { params, ret, body, .. } | NodeKind::Method { params, ret, body, .. } => {
            params.iter().any(|p| p.ty.contains("Path")) || ret.contains("Path") || matches!(body, model::ir::node::Body::Raw(b) if b.contains("Path::"))
        }
        _ => false,
    }
}

fn contains_symbol_use(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Struct { fields, .. } => fields.iter().any(|f| f.ty.contains("Symbol")),
        NodeKind::Function { params, ret, .. } | NodeKind::Method { params, ret, .. } => {
            params.iter().any(|p| p.ty.contains("Symbol")) || ret.contains("Symbol")
        }
        _ => false,
    }
}

fn dedup_uses(items: &mut Vec<ItemPlan>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    items.retain(|item| {
        if let ItemPlan::Leaf(NodeKind::Use { vis, path, alias, glob }) = item {
            let key = format!("{:?}|{}|{}|{}", vis, path, alias.as_deref().unwrap_or(""), glob);
            return seen.insert(key);
        }
        true
    });
}
