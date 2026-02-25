//! Iterative dominator computation over a control-flow graph.

use std::collections::{HashMap, HashSet};

/// Post-dominator computation.
///
/// Variables:
///   node_count : usize              — number of real blocks
///   succs      : &[Vec<usize>]      — forward adjacency (succs[v] = successors)
///   exit_nodes : &[usize]           — blocks whose Terminator is Return
///
/// Equations:
///   G_rev : reverse all edges of the forward CFG
///   super_exit = node_count        (synthetic node index)
///   G_rev edges from super_exit -> each exit_node
///   post_dom[v] = dom(G_rev, super_exit)[v]
///
/// Returns post_dom[v] = immediate post-dominator index for each real block v.
/// super_exit node (index node_count) is included in returned vec but can be ignored.
pub fn post_dominators(node_count: usize, succs: &[Vec<usize>], exit_nodes: &[usize]) -> Vec<HashSet<usize>> {
    let total = node_count + 1; // +1 for synthetic super_exit
    let super_exit = node_count;

    // Build reversed predecessor map for the iterative dominator algorithm.
    // In reversed graph: edges go dst->src for every src->dst in forward graph.
    // super_exit -> each real exit block.
    let mut preds: HashMap<usize, Vec<usize>> = HashMap::new();
    for v in 0..node_count {
        let nbrs = succs.get(v).map(|s| s.as_slice()).unwrap_or(&[]);
        for &w in nbrs {
            // forward edge v->w  =>  reversed edge w->v, so w has pred v in rev graph
            preds.entry(w).or_default().push(v);
        }
    }
    // super_exit -> each exit_node in forward = exit_node -> super_exit in rev
    for &e in exit_nodes {
        preds.entry(e).or_default().push(super_exit);
    }

    dominators(total, &preds, super_exit)
}

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
