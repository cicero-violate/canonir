use anyhow::Result;
use canon::ir::CanonIR;
use canon::node::{CanonNodeKind, NameId};
use std::collections::HashMap;

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
    let mut crate_name = String::new();
    let mut declared_packages: HashMap<String, Option<String>> = HashMap::new();
    for n in &ir.nodes {
        if let CanonNodeKind::Crate {
            name_id,
            declared_dependencies,
            ..
        } = &n.kind
        {
            crate_name = ir.lookup_name(*name_id).to_string();
            for dep in declared_dependencies {
                let root = ir.lookup_path(dep.crate_root).to_string();
                let pkg = dep.package_name.map(|nid| ir.lookup_name(nid).to_string());
                declared_packages.insert(root, pkg);
            }
            break;
        }
    }
    // Gather all external crate roots referenced by Use nodes.
    let mut extern_roots: Vec<String> = Vec::new();
    let mut push_root = |root: &str| {
        if root.is_empty() {
            return;
        }
        if BUILTIN_ROOTS.contains(&root) {
            return;
        }
        if root == crate_name.as_str() {
            return;
        }
        if !extern_roots.iter().any(|r| r == root) {
            extern_roots.push(root.to_string());
        }
    };

    for node in &ir.nodes {
        if let CanonNodeKind::Use { path_id, .. } = &node.kind {
            let path = ir.lookup_path(*path_id);
            let root = path.split("::").next().unwrap_or("").trim();
            push_root(root);
        }
        if let CanonNodeKind::PathRef { path_id } = &node.kind {
            let path = ir.lookup_path(*path_id);
            let root = path.split("::").next().unwrap_or("").trim();
            push_root(root);
        }
    }

    if extern_roots.is_empty() {
        return Ok(());
    }

    // Intern each root and write into the Crate node fields.
    let mut path_ids = Vec::new();
    let mut package_ids: Vec<Option<NameId>> = Vec::new();
    for root in &extern_roots {
        path_ids.push(ir.intern_path(root));
        let pkg_id = declared_packages
            .get(root)
            .and_then(|p| p.as_deref())
            .map(|pkg| NameId(ir.name_intern.intern(pkg)));
        package_ids.push(pkg_id);
    }

    for node in ir.nodes.iter_mut() {
        if let CanonNodeKind::Crate {
            dependencies,
            dependency_packages,
            ..
        } = &mut node.kind
        {
            *dependencies = path_ids.clone();
            *dependency_packages = package_ids.clone();
            break;
        }
    }

    Ok(())
}
