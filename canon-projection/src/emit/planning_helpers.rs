use std::collections::{HashMap, HashSet, VecDeque};

// ---------------- Dependency Graph ----------------

#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub edges: HashMap<String, HashSet<String>>, // item -> dependencies
}

pub fn compute_dependency_graph(items: &[(String, Vec<String>)]) -> DependencyGraph {
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

    for (item, deps) in items.iter() {
        edges
            .entry(item.clone())
            .or_insert_with(HashSet::new)
            .extend(deps.iter().cloned());
    }

    DependencyGraph { edges }
}

// ---------------- Emit Order ----------------

pub fn compute_emit_order(graph: &DependencyGraph) -> Result<Vec<String>, String> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    for (node, deps) in graph.edges.iter() {
        in_degree.entry(node.clone()).or_insert(0);
        for dep in deps {
            *in_degree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(n, _)| n.clone())
        .collect();

    let mut order = Vec::new();

    while let Some(node) = queue.pop_front() {
        order.push(node.clone());

        if let Some(deps) = graph.edges.get(&node) {
            for dep in deps {
                if let Some(d) = in_degree.get_mut(dep) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    if order.len() != in_degree.len() {
        return Err("cycle detected in dependency graph".to_string());
    }

    Ok(order)
}

// ---------------- Module Normalization ----------------

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub name: String,
    pub children: Vec<ModuleNode>,
}

pub fn normalize_module_tree(modules: &[String]) -> ModuleNode {
    let mut root = ModuleNode {
        name: "crate".to_string(),
        children: Vec::new(),
    };

    for module_path in modules {
        let parts: Vec<&str> = module_path.split("::").collect();
        insert_module(&mut root, &parts);
    }

    root
}

fn insert_module(node: &mut ModuleNode, parts: &[&str]) {
    if parts.is_empty() {
        return;
    }

    let name = parts[0];

    let child = node
        .children
        .iter_mut()
        .find(|c| c.name == name);

    if let Some(child_node) = child {
        insert_module(child_node, &parts[1..]);
    } else {
        let mut new_node = ModuleNode {
            name: name.to_string(),
            children: Vec::new(),
        };

        insert_module(&mut new_node, &parts[1..]);
        node.children.push(new_node);
    }
}

// ---------------- Structural Validation ----------------

#[derive(Debug)]
pub struct StructuralError {
    pub message: String,
}

pub fn validate_structure(items: &[String]) -> Vec<StructuralError> {
    let mut errors = Vec::new();
    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item.clone()) {
            errors.push(StructuralError {
                message: format!("duplicate item definition: {}", item),
            });
        }
    }

    errors
}

// ---------------- Unresolved Use Detection ----------------

pub fn detect_unresolved_paths(
    uses: &[(String, String)], // (file, path)
    symbol_table: &HashSet<String>,
) -> Vec<String> {
    let mut unresolved = Vec::new();

    for (file, path) in uses {
        if !symbol_table.contains(path) {
            unresolved.push(format!("{}: unresolved import {}", file, path));
        }
    }

    unresolved
}
