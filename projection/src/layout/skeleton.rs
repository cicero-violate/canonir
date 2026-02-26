use std::path::PathBuf;

use model::ir::{
    edge::EdgeKind,
    model_ir::ModelIR,
    node::{Body, NodeId, NodeKind},
};

use crate::layout::{FilePlan, ImplPlan, ItemPlan, ModuleDeclPlan, Plan};

/// Build the raw structural plan directly from IR (no heuristics or mutations).
pub fn plan_from_ir(ir: &ModelIR) -> Plan {
    let mut files: Vec<FilePlan> = Vec::new();
    let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

    for n in &ir.nodes {
        if let NodeKind::Module { inline, .. } = &n.kind {
            if !inline {
                let default_public = if let NodeKind::Module { file, .. } = &n.kind {
                    !file.ends_with("main.rs")
                } else {
                    true
                };
                let items = collect_module_items(ir, n.id, &mut seen, default_public);
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
        let dependencies = if ir.cargo_dependencies.is_empty() {
            infer_dependencies(ir)
        } else {
            ir.cargo_dependencies.clone()
        };
        files.push(FilePlan {
            path: PathBuf::from("Cargo.toml"),
            items: vec![ItemPlan::CargoToml {
                name,
                edition,
                has_binary,
                dependencies,
            }],
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Plan { files }
}

fn infer_dependencies(ir: &ModelIR) -> Vec<String> {
    let mut crates: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let crate_name = ir.nodes.iter().find_map(|n| match &n.kind {
        NodeKind::Crate { name, .. } => Some(name.as_str()),
        _ => None,
    });
    for n in &ir.nodes {
        if let NodeKind::Use { path, .. } = &n.kind {
            push_dep_root(&mut crates, path, crate_name);
        }
        match &n.kind {
            NodeKind::Function { body, .. } | NodeKind::Method { body, .. } => {
                if let Body::Raw(src) = body {
                    for root in roots_from_text(src) {
                        push_dep_root(&mut crates, &root, crate_name);
                    }
                }
            }
            _ => {}
        }
    }
    crates.into_iter().map(|name| format!("{name} = \"*\"")).collect()
}

fn push_dep_root(out: &mut std::collections::BTreeSet<String>, path: &str, crate_name: Option<&str>) {
    let root = path.split("::").next().unwrap_or("");
    if root.is_empty() || matches!(root, "crate" | "self" | "super" | "std" | "core" | "alloc") {
        return;
    }
    if crate_name.is_some_and(|name| name == root) {
        return;
    }
    if !root.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return;
    }
    if root.chars().any(|c| c.is_ascii_uppercase()) {
        return;
    }
    if matches!(
        root,
        "env" | "fs" | "fmt" | "path" | "error" | "result" | "option" | "vec" | "string"
            | "collections" | "cmp" | "marker" | "future" | "iter" | "hash" | "io"
    ) {
        return;
    }
    out.insert(root.replace('_', "-"));
}

fn roots_from_text(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i + 1 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b':' {
                if start > 0 && bytes[start - 1] == b':' {
                    continue;
                }
                if i + 2 < bytes.len() && bytes[i + 2] == b'<' {
                    continue;
                }
                out.push(src[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

fn collect_module_items(
    ir: &ModelIR,
    module_id: NodeId,
    seen: &mut std::collections::HashSet<NodeId>,
    default_public: bool,
) -> Vec<ItemPlan> {
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
            NodeKind::Module { path, vis, inline, .. } => {
                let name = path.rsplit("::").next().unwrap_or(path.as_str()).to_string();
                if *inline {
                    let nested = collect_module_items(ir, child_id, seen, default_public);
                    items.push(ItemPlan::Module(ModuleDeclPlan {
                        name,
                        vis: vis.clone(),
                        default_public,
                        inline: true,
                        items: nested,
                        node_id: Some(child_id),
                    }));
                } else {
                    items.push(ItemPlan::Module(ModuleDeclPlan {
                        name,
                        vis: vis.clone(),
                        default_public,
                        inline: false,
                        items: Vec::new(),
                        node_id: Some(child_id),
                    }));
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
