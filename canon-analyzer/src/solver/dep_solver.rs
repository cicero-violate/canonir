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
