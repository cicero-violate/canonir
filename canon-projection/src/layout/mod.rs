use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use canon::ir::CanonIR;
use canon::node::{flags, CanonId, CanonNodeKind};

#[derive(Debug, Clone)]
pub struct Plan {
    pub files: Vec<FilePlan>,
}

#[derive(Debug, Clone)]
pub struct FilePlan {
    pub path: PathBuf,
    pub items: Vec<ItemPlan>,
}

#[derive(Debug, Clone)]
pub enum ItemPlan {
    CargoToml { name: String, edition: String, has_binary: bool, dependencies: Vec<String> },
    Node(CanonId),
}

// Internal kinds that should never appear as top-level file items.
fn is_internal(kind: &CanonNodeKind) -> bool {
    matches!(
        kind,
        CanonNodeKind::Crate { .. }
            | CanonNodeKind::Type { .. }
            | CanonNodeKind::FnSig { .. }
            | CanonNodeKind::Field { .. }
            | CanonNodeKind::Param { .. }
            | CanonNodeKind::GenericParam { .. }
            | CanonNodeKind::WherePred { .. }
            | CanonNodeKind::Variant { .. }
            | CanonNodeKind::Attr { .. }
            | CanonNodeKind::Lifetime { .. }
            | CanonNodeKind::Body { .. }
            | CanonNodeKind::BasicBlock { .. }
            | CanonNodeKind::Local { .. }
    )
}

pub fn build_plan(ir: &CanonIR) -> Result<Plan> {
    let mut files = Vec::new();

    // Find the root module (path == "crate").
    let root_mod = ir.nodes.iter().find(|n| if let CanonNodeKind::Module { path_id, .. } = &n.kind { ir.lookup_path(*path_id) == "crate" } else { false });

    if let Some(root) = root_mod {
        let has_root_main = module_children(ir, root.id).iter().any(|id| is_root_main_fn(ir, *id));
        let root_file = if has_root_main { PathBuf::from("src/main.rs") } else { PathBuf::from("src/lib.rs") };
        walk_module(ir, root.id, root_file, &mut files);
    } else {
        // Fallback: flat emit of all root-level items.
        let items = flat_root_items(ir);
        files.push(FilePlan { path: PathBuf::from("src/lib.rs"), items: items.into_iter().map(ItemPlan::Node).collect() });
    }

    if let Some((name, edition)) = crate_meta(ir) {
        files.push(FilePlan { path: PathBuf::from("Cargo.toml"), items: vec![ItemPlan::CargoToml { name, edition, has_binary: false, dependencies: infer_dependencies(ir) }] });
    }

    Ok(Plan { files })
}

/// Recursively walk a module node, producing a FilePlan for it and all
/// non-inline child modules.
///
/// `file_path` is the output path for this module (e.g. `src/lib.rs`,
/// `src/foo.rs`, `src/foo/mod.rs`).
fn walk_module(ir: &CanonIR, module_id: CanonId, file_path: PathBuf, files: &mut Vec<FilePlan>) {
    let is_inline = {
        if let CanonNodeKind::Module { flags: f, .. } = &ir.node(module_id).kind {
            (*f & flags::INLINE) != 0
        } else {
            false
        }
    };
    let _ = is_inline; // used by caller context; this module always gets its own file here

    // Collect direct children of this module via module_graph Contains edges,
    // preserving emit_order where possible.
    let children = module_children(ir, module_id);

    // Determine the directory prefix for child module files.
    // src/lib.rs  → children go in src/
    // src/foo.rs  → if foo has sub-modules, it must become src/foo/mod.rs
    // We handle this by computing child paths relative to this file's stem dir.
    let stem_dir = module_stem_dir(&file_path);

    let mut items: Vec<ItemPlan> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();

    for child_id in &children {
        let idx = child_id.0 as usize;
        if idx >= ir.nodes.len() || seen.contains(&child_id.0) {
            continue;
        }
        seen.insert(child_id.0);

        let kind = &ir.nodes[idx].kind;
        if is_internal(kind) {
            continue;
        }

        if let CanonNodeKind::Module { path_id, flags: f } = kind {
            let inline = (*f & flags::INLINE) != 0;
            if inline {
                // Inline module: emit it inline in this file.
                items.push(ItemPlan::Node(*child_id));
            } else {
                // Non-inline module: emit `mod name;` in this file,
                // and recursively produce a new file for it.
                items.push(ItemPlan::Node(*child_id));
                let mod_name = module_name(ir, *path_id);
                // Check if this sub-module itself has child modules
                // to decide between foo.rs and foo/mod.rs.
                let grandchildren = module_children(ir, *child_id);
                let has_sub_modules = grandchildren.iter().any(|gc| matches!(ir.nodes[gc.0 as usize].kind, CanonNodeKind::Module { flags: f, .. } if (f & flags::INLINE) == 0));
                let child_path = if has_sub_modules { stem_dir.join(format!("{}/mod.rs", mod_name)) } else { stem_dir.join(format!("{}.rs", mod_name)) };
                walk_module(ir, *child_id, child_path, files);
            }
        } else {
            items.push(ItemPlan::Node(*child_id));
        }
    }

    files.push(FilePlan { path: file_path, items });
}

/// Returns the directory that sibling/child source files live in,
/// given a module's output file path.
///
/// src/lib.rs      → src/
/// src/foo.rs      → src/foo/   (if promoted to mod.rs, caller uses stem_dir directly)
/// src/foo/mod.rs  → src/foo/
fn module_stem_dir(file_path: &PathBuf) -> PathBuf {
    let file_name = file_path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    if file_name == "lib.rs" || file_name == "mod.rs" {
        file_path.parent().unwrap_or(file_path).to_path_buf()
    } else {
        // foo.rs → treat children as foo/
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("mod");
        file_path.parent().unwrap_or(file_path).join(stem)
    }
}

/// Extract the leaf module name from a path_id (e.g. "crate::foo::bar" → "bar").
fn module_name<'a>(ir: &'a CanonIR, path_id: canon::node::PathId) -> &'a str {
    let path = ir.lookup_path(path_id);
    path.rsplit("::").next().unwrap_or(path)
}

/// Return direct children of a module via module_graph Contains edges,
/// in emit_order where possible.
fn module_children(ir: &CanonIR, module_id: CanonId) -> Vec<CanonId> {
    // Build set of direct children via Contains edges.
    let src = canon::id::NodeId(module_id.0);
    let mut child_set: HashSet<u32> = HashSet::new();
    for (dst, edge) in ir.module_graph.neighbours(src) {
        if matches!(edge, canon::edge::EdgeKind::Contains) {
            child_set.insert(dst.0);
        }
    }

    if child_set.is_empty() {
        return Vec::new();
    }

    // Return in emit_order if available, else by node id.
    if !ir.emit_order.is_empty() {
        let mut result: Vec<CanonId> = ir.emit_order.iter().filter(|id| child_set.contains(&id.0)).copied().collect();
        // Append any children not in emit_order (shouldn't happen, but be safe).
        for id in &child_set {
            if !ir.emit_order.iter().any(|e| e.0 == *id) {
                result.push(CanonId(*id));
            }
        }
        result
    } else {
        let mut v: Vec<CanonId> = child_set.into_iter().map(CanonId).collect();
        v.sort_by_key(|id| id.0);
        v
    }
}

fn is_root_main_fn(ir: &CanonIR, id: CanonId) -> bool {
    match &ir.node(id).kind {
        CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id) == "main",
        _ => false,
    }
}

fn crate_meta(ir: &CanonIR) -> Option<(String, String)> {
    ir.nodes.iter().find_map(|n| if let CanonNodeKind::Crate { name_id, edition } = &n.kind { Some((ir.lookup_name(*name_id).to_string(), edition.to_string())) } else { None })
}

fn infer_dependencies(ir: &CanonIR) -> Vec<String> {
    let mut deps = std::collections::BTreeSet::new();
    let crate_name = crate_meta(ir).map(|(name, _)| name);
    for n in &ir.nodes {
        if let CanonNodeKind::Use { path_id, .. } = &n.kind {
            let path = ir.lookup_path(*path_id);
            let root = path.split("::").next().unwrap_or("").trim_start_matches('&').trim_start_matches(':');
            if root.is_empty() {
                continue;
            }
            if !root.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                continue;
            }
            if matches!(root, "crate" | "self" | "super" | "std" | "core" | "alloc") {
                continue;
            }
            if crate_name.as_deref().is_some_and(|n| n == root) {
                continue;
            }
            deps.insert(root.replace('_', "-"));
        }
    }
    // Scan interned/raw text for fully-qualified external paths
    // (e.g. tree_sitter_rust::LANGUAGE) used directly in bodies.
    for s in &ir.name_intern.vec {
        for root in roots_from_text(s) {
            if matches!(root.as_str(), "crate" | "self" | "super" | "std" | "core" | "alloc") {
                continue;
            }
            if !root.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                continue;
            }
            if crate_name.as_deref().is_some_and(|n| n == root) {
                continue;
            }
            deps.insert(root.replace('_', "-"));
        }
    }
    deps.into_iter().map(|d| format!("{} = \"*\"", d)).collect()
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
                out.push(src[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Fallback flat emit when no root module is found.
fn flat_root_items(ir: &CanonIR) -> Vec<CanonId> {
    let mut has_parent = vec![false; ir.nodes.len()];
    for src in 0..ir.module_graph.vertex_count() {
        let src_id = canon::id::NodeId(src as u32);
        for (dst, edge) in ir.module_graph.neighbours(src_id) {
            if matches!(edge, canon::edge::EdgeKind::Contains) && dst.index() < has_parent.len() {
                has_parent[dst.index()] = true;
            }
        }
    }
    let emit: Vec<CanonId> = if ir.emit_order.is_empty() { ir.nodes.iter().map(|n| n.id).collect() } else { ir.emit_order.clone() };
    let mut seen: HashSet<u32> = HashSet::new();
    let mut items = Vec::new();
    for id in emit {
        let idx = id.0 as usize;
        if idx >= ir.nodes.len() || seen.contains(&id.0) || has_parent[idx] {
            continue;
        }
        if is_internal(&ir.nodes[idx].kind) {
            continue;
        }
        seen.insert(id.0);
        items.push(id);
    }
    items
}
