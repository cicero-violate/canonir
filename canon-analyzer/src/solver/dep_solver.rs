use anyhow::Result;
use canon::ir::CanonIR;
use canon::node::CanonNodeKind;

/// Stdlib / language pseudo-crate roots that must not appear as Cargo dependencies.
const BUILTIN_ROOTS: &[&str] = &[
    "std", "core", "alloc", "proc_macro",
    "crate", "self", "super",
];

/// Populate `Crate.dependencies` from the Use nodes present in the IR.
///
/// For every `CanonNodeKind::Use { path_id, .. }` the solver:
/// 1. Extracts the leading path segment (e.g. `"serde"` from `"serde::Deserialize"`).
/// 2. Skips builtin roots and single-segment paths that equal the crate name.
/// 3. Interns the crate root as a `PathId` and appends it to `Crate.dependencies`
///    (deduplicating by value).
///
/// This is the canonical replacement for `infer_dependencies` / `roots_from_text`
/// which were deleted from `layout/mod.rs` as part of g1.
pub fn solve(ir: &mut CanonIR) -> Result<()> {
    // Collect the crate name so we can skip self-references.
    let crate_name: String = ir
        .nodes
        .iter()
        .find_map(|n| {
            if let CanonNodeKind::Crate { name_id, .. } = &n.kind {
                Some(ir.lookup_name(*name_id).to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let local_module_roots: std::collections::HashSet<String> = ir
        .nodes
        .iter()
        .filter_map(|n| {
            if let CanonNodeKind::Module { path_id, .. } = &n.kind {
                ir.lookup_path(*path_id).strip_prefix("crate::").and_then(|rest| rest.split("::").next()).map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    // Gather all external crate roots referenced by Use nodes.
    let mut extern_roots: Vec<String> = Vec::new();
    for node in &ir.nodes {
        if let CanonNodeKind::Use { path_id, .. } = &node.kind {
            let path = ir.lookup_path(*path_id);
            let root = path.split("::").next().unwrap_or("").trim();
            if root.is_empty() {
                continue;
            }
            if BUILTIN_ROOTS.contains(&root) {
                continue;
            }
            if root == crate_name.as_str() {
                continue;
            }
            if local_module_roots.contains(root) {
                continue;
            }
            if !is_probable_crate_name(root) {
                continue;
            }
            if !extern_roots.iter().any(|r| r == root) {
                extern_roots.push(root.to_string());
            }
        }
    }

    // Fallback extraction for explicit external crate paths in raw code snippets
    // (e.g. `tree_sitter_rust::LANGUAGE`) that do not appear as Use nodes.
    for text in &ir.name_intern.vec {
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':')) {
            let Some((root, rest)) = token.split_once("::") else {
                continue;
            };
            let Some(first_rest) = rest.chars().next() else {
                continue;
            };
            if !(first_rest.is_ascii_alphabetic() || first_rest == '_') {
                continue;
            }
            if root.is_empty() || BUILTIN_ROOTS.contains(&root) || root == crate_name.as_str() {
                continue;
            }
            if local_module_roots.contains(root) {
                continue;
            }
            if !is_probable_crate_name(root) {
                continue;
            }
            if !extern_roots.iter().any(|r| r == root) {
                extern_roots.push(root.to_string());
            }
        }
    }

    if extern_roots.is_empty() {
        return Ok(());
    }

    // Intern each root and write into the Crate node's dependencies field.
    let path_ids: Vec<canon::node::PathId> = extern_roots
        .iter()
        .map(|r| ir.intern_path(r))
        .collect();

    for node in ir.nodes.iter_mut() {
        if let CanonNodeKind::Crate { dependencies, .. } = &mut node.kind {
            for pid in &path_ids {
                if !dependencies.contains(pid) {
                    dependencies.push(*pid);
                }
            }
            break;
        }
    }

    Ok(())
}

fn is_probable_crate_name(root: &str) -> bool {
    let mut chars = root.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
