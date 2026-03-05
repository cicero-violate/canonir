use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Compute deterministic dependency ordering using a simple topological sort.
pub fn topo_sort(graph: &HashMap<String, Vec<String>>) -> Result<Vec<String>, String> {
    let mut indegree: HashMap<String, usize> = HashMap::new();

    for (node, deps) in graph {
        indegree.entry(node.clone()).or_insert(0);
        for dep in deps {
            *indegree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut ready: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| n.clone())
        .collect();

    ready.sort();

    let mut out = Vec::new();

    while let Some(node) = ready.pop() {
        out.push(node.clone());

        if let Some(edges) = graph.get(&node) {
            for dep in edges {
                if let Some(v) = indegree.get_mut(dep) {
                    *v -= 1;
                    if *v == 0 {
                        ready.push(dep.clone());
                        ready.sort();
                    }
                }
            }
        }
    }

    if out.len() != indegree.len() {
        return Err("cycle detected in dependency graph".into());
    }

    Ok(out)
}

/// Detect duplicate emitted symbols.
pub fn detect_duplicates(items: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();

    for item in items {
        if !seen.insert(item) {
            return Err(format!("duplicate symbol emitted: {}", item));
        }
    }

    Ok(())
}

/// Build a deterministic module -> blocks emission plan.
pub fn build_emit_plan(items: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut plan: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (module, code) in items {
        plan.entry(module.clone()).or_default().push(code.clone());
    }

    for blocks in plan.values_mut() {
        blocks.sort();
    }

    plan
}

/// Normalize module paths into a deterministic module set.
pub fn normalize_modules(items: &[(String, String)]) -> BTreeSet<String> {
    let mut modules = BTreeSet::new();

    for (module, _) in items {
        if module.trim().is_empty() {
            modules.insert("root".to_string());
            continue;
        }

        let mut path = String::new();
        for part in module.split("::") {
            if !path.is_empty() {
                path.push_str("::");
            }
            path.push_str(part);
            modules.insert(path.clone());
        }
    }

    modules
}
