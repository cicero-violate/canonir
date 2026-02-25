use std::path::PathBuf;

use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{NodeId, NodeKind},
};

use crate::layout::{FilePlan, ImplPlan, ItemPlan, ModuleDeclPlan, Plan};

/// Build the raw structural plan directly from IR (no heuristics or mutations).
pub fn plan_from_ir(ir: &ModelIR) -> Plan {
    let mut files: Vec<FilePlan> = Vec::new();
    let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for n in &ir.nodes {
        if let NodeKind::Module { inline, .. } = &n.kind {
            if !inline {
                let items = collect_module_items(ir, n.id, &mut seen);
                if let NodeKind::Module { file, .. } = &n.kind {
                    files.push(FilePlan { path: PathBuf::from(file), items });
                }
            }
        }
    }

    // Cargo.toml entry
    if let Some((name, edition)) = ir.nodes.iter().find_map(|n| match &n.kind {
        NodeKind::Crate { name, edition } => Some((name.clone(), edition.clone())),
        _ => None,
    }) {
        let has_binary = ir.nodes.iter().any(|n| match &n.kind {
            NodeKind::Module { file, .. } => file.ends_with("main.rs"),
            _ => false,
        });
        files.push(FilePlan { path: PathBuf::from("Cargo.toml"), items: vec![ItemPlan::CargoToml { name, edition, has_binary }] });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Plan { files }
}

fn collect_module_items(ir: &ModelIR, module_id: NodeId, seen: &mut std::collections::HashSet<NodeId>) -> Vec<ItemPlan> {
    if !seen.insert(module_id) {
        return Vec::new();
    }
    let mut items: Vec<ItemPlan> = Vec::new();

    for (child_id, edge) in ir.module_graph.neighbours(module_id) {
        if *edge != EdgeKind::Contains {
            continue;
        }
        let child = ir.node(child_id);
        match &child.kind {
            NodeKind::Use { .. }
            | NodeKind::Function { .. }
            | NodeKind::Method { .. }
            | NodeKind::Struct { .. }
            | NodeKind::Enum { .. }
            | NodeKind::Trait { .. }
            | NodeKind::TypeAlias { .. }
            | NodeKind::Const { .. }
            | NodeKind::Static { .. }
            | NodeKind::MacroCall { .. }
            | NodeKind::TypeRef { .. }
            | NodeKind::ExternCrate { .. }
            | NodeKind::Lifetime { .. }
            | NodeKind::Crate { .. } => {
                items.push(ItemPlan::Leaf(child.kind.clone()));
            }
            NodeKind::Module { path, inline, .. } => {
                let name = path.rsplit("::").next().unwrap_or(path.as_str()).to_string();
                if *inline {
                    let nested = collect_module_items(ir, child_id, seen);
                    items.push(ItemPlan::Module(ModuleDeclPlan { name, inline: true, items: nested, node_id: Some(child_id) }));
                } else {
                    items.push(ItemPlan::Module(ModuleDeclPlan { name, inline: false, items: Vec::new(), node_id: Some(child_id) }));
                }
            }
            NodeKind::Impl { for_struct, for_trait, generics, attrs, where_clauses, unsafe_ } => {
                items.push(ItemPlan::Impl(ImplPlan {
                    node_id: Some(child_id),
                    for_struct: for_struct.clone(),
                    for_trait: for_trait.clone(),
                    generics: generics.clone(),
                    attrs: attrs.clone(),
                    where_clauses: where_clauses.clone(),
                    unsafe_: *unsafe_,
                    methods: Vec::new(),
                }));
            }
        }
    }

    items
}
