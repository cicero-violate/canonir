//! Module Tree Normalization Utilities
//!
//! These helpers construct a deterministic Rust module tree from CanonIR
//! emission plans. The normalization pass ensures:
//! - stable ordering of modules and files
//! - no duplicate module definitions
//! - consistent `mod` declarations
//! - deterministic filesystem layout

use std::collections::{BTreeMap, BTreeSet};

/// Represents a normalized module node.
#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub name: String,
    pub children: BTreeMap<String, ModuleNode>,
    pub files: BTreeSet<String>,
}

impl ModuleNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            children: BTreeMap::new(),
            files: BTreeSet::new(),
        }
    }

    /// Insert a module path such as `foo::bar::baz`.
    pub fn insert_module_path(&mut self, path: &[String]) {
        if path.is_empty() {
            return;
        }

        let head = &path[0];
        let tail = &path[1..];

        let child = self
            .children
            .entry(head.clone())
            .or_insert_with(|| ModuleNode::new(head.clone()));

        child.insert_module_path(tail);
    }

    /// Insert a file under a module path.
    pub fn insert_file(&mut self, module_path: &[String], file: String) {
        if module_path.is_empty() {
            self.files.insert(file);
            return;
        }

        let head = &module_path[0];
        let tail = &module_path[1..];

        let child = self
            .children
            .entry(head.clone())
            .or_insert_with(|| ModuleNode::new(head.clone()));

        child.insert_file(tail, file);
    }
}

/// Normalize a set of module paths into a deterministic tree.
pub fn normalize_module_tree(paths: Vec<Vec<String>>) -> ModuleNode {
    let mut root = ModuleNode::new("crate");

    for p in paths {
        root.insert_module_path(&p);
    }

    root
}

/// Collect ordered module declarations (`mod foo;`).
pub fn collect_mod_decls(node: &ModuleNode) -> Vec<String> {
    node.children
        .keys()
        .map(|name| format!("mod {};", name))
        .collect()
}

/// Recursively walk the module tree and produce deterministic traversal order.
pub fn walk_modules(node: &ModuleNode, out: &mut Vec<String>, prefix: String) {
    for (name, child) in &node.children {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}::{}", prefix, name)
        };

        out.push(path.clone());
        walk_modules(child, out, path);
    }
}
