use std::collections::{HashMap, VecDeque};
use crate::config::Task;

pub fn topo_sort(tasks: &[Task]) -> Result<Vec<String>, String> {
    let mut indeg = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for t in tasks {
        indeg.insert(t.name.clone(), 0);
        adj.insert(t.name.clone(), vec![]);
    }

    for t in tasks {
        for d in &t.depends_on {
            *indeg.get_mut(&t.name).unwrap() += 1;
            adj.get_mut(d).unwrap().push(t.name.clone());
        }
    }

    let mut q = VecDeque::new();
    for (k, v) in &indeg {
        if *v == 0 { q.push_back(k.clone()); }
    }

    let mut out = vec![];
    while let Some(n) = q.pop_front() {
        out.push(n.clone());
        for nxt in adj.get(&n).unwrap() {
            let e = indeg.get_mut(nxt).unwrap();
            *e -= 1;
            if *e == 0 { q.push_back(nxt.clone()); }
        }
    }

    if out.len() != tasks.len() {
        return Err("cycle detected".into());
    }

    Ok(out)
}

