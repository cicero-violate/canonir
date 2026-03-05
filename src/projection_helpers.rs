use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct ItemNode {
    pub id: String,
    pub module: String,
    pub deps: Vec<String>,
}

pub type DependencyGraph = HashMap<String, ItemNode>;

pub fn compute_dependency_graph(items: &[ItemNode]) -> DependencyGraph {
    let mut graph = HashMap::new();
    for item in items {
        graph.insert(item.id.clone(), item.clone());
    }
    graph
}

pub fn compute_emit_order(graph: &DependencyGraph) -> Result<Vec<String>, String> {
    let mut indegree: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for (id, node) in graph {
        indegree.entry(id.clone()).or_insert(0);
        for dep in &node.deps {
            adj.entry(dep.clone()).or_default().push(id.clone());
            *indegree.entry(id.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<String> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(k, _)| k.clone())
        .collect();

    let mut ordered = Vec::new();

    while let Some(node) = queue.pop_front() {
        ordered.push(node.clone());

        if let Some(neigh) = adj.get(&node) {
            for n in neigh {
                if let Some(e) = indegree.get_mut(n) {
                    *e -= 1;
                    if *e == 0 {
                        queue.push_back(n.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != graph.len() {
        return Err("cycle detected in dependency graph".into());
    }

    Ok(ordered)
}

pub fn normalize_modules(items: &[ItemNode]) -> HashMap<String, Vec<String>> {
    let mut modules: HashMap<String, Vec<String>> = HashMap::new();

    for item in items {
        modules
            .entry(item.module.clone())
            .or_default()
            .push(item.id.clone());
    }

    modules
}

pub fn validate_structure(items: &[ItemNode]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();

    for item in items {
        if !ids.insert(item.id.clone()) {
            errors.push(format!("duplicate definition: {}", item.id));
        }
    }

    let id_set: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();

    for item in items {
        for dep in &item.deps {
            if !id_set.contains(dep) {
                errors.push(format!(
                    "unresolved dependency: {} -> {}",
                    item.id, dep
                ));
            }
        }
    }

    errors
}

pub fn plan_emission(items: &[ItemNode]) -> Result<Vec<ItemNode>, String> {
    let graph = compute_dependency_graph(items);
    let order = compute_emit_order(&graph)?;

    let map: HashMap<String, ItemNode> = items
        .iter()
        .cloned()
        .map(|i| (i.id.clone(), i))
        .collect();

    let mut result = Vec::new();
    for id in order {
        if let Some(item) = map.get(&id) {
            result.push(item.clone());
        }
    }

    Ok(result)
}
