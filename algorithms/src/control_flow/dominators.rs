//! Iterative dominator computation over a control-flow graph.

use std::collections::{HashMap, HashSet};

pub fn dominators(node_count: usize, preds: &HashMap<usize, Vec<usize>>, entry: usize) -> Vec<HashSet<usize>> {
    let all: HashSet<usize> = (0..node_count).collect();
    let mut dom: Vec<HashSet<usize>> = vec![all.clone(); node_count];
    if entry < node_count {
        dom[entry] = std::iter::once(entry).collect();
    }

    let mut changed = true;
    while changed {
        changed = false;
        for n in 0..node_count {
            if n == entry {
                continue;
            }
            let pred_list = preds.get(&n).cloned().unwrap_or_default();
            let mut new_dom = if pred_list.is_empty() {
                all.clone()
            } else {
                let mut iter = pred_list.iter().map(|p| dom[*p].clone());
                let first = iter.next().unwrap_or_default();
                iter.fold(first, |acc, s| acc.intersection(&s).cloned().collect())
            };
            new_dom.insert(n);
            if new_dom != dom[n] {
                dom[n] = new_dom;
                changed = true;
            }
        }
    }
    dom
}
