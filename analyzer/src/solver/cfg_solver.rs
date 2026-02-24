//! CFG solver — dominator computation + unreachable block detection.
//!
//! Variables:
//!   G_cfg      = ir.cfg_graph         (CSR, per-function CFG edges)
//!   dom[v]     = immediate dominator of block v
//!   reachable  = DFS(G_cfg, entry=0)
//!   dead_blocks = V \ reachable
//!
//! Equations:
//!   reachable   = DFS(G_cfg, 0)
//!   dead_blocks = { v | v not in reachable }
//!
//! Dominator equation (Cooper et al. iterative):
//!   dom[entry] = entry
//!   dom[v]     = intersect{ dom[p] | p in preds(v) } ∪ {v}
//!
//! Algorithm used: DFS for reachability (algorithms crate),
//!                 iterative dominator solve (local, standard)

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use algorithms::graph::dfs::dfs;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.cfg_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj = csr_to_adj(&ir.cfg_graph);

    // Reachability from entry block 0.
    let reached = dfs(&adj, 0);
    let reachable: std::collections::HashSet<usize> = reached.into_iter().collect();
    let _dead: Vec<usize> = (0..v).filter(|i| !reachable.contains(i)).collect();

    // Iterative dominator computation (Cooper et al.).
    // dom[v] = index of immediate dominator, UNDEFINED = usize::MAX.
    let undef = usize::MAX;
    let mut dom = vec![undef; v];
    dom[0] = 0;

    // Build reverse adjacency (predecessors).
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); v];
    for (u, nbrs) in adj.iter().enumerate() {
        for &w in nbrs { pred[w].push(u); }
    }

    // Post-order from DFS for convergence speed.
    let mut changed = true;
    while changed {
        changed = false;
        // Process in reverse post-order (skip entry).
        for v_node in 1..v {
            if !reachable.contains(&v_node) { continue; }
            let processed_preds: Vec<usize> = pred[v_node]
                .iter()
                .filter(|&&p| dom[p] != undef)
                .copied()
                .collect();
            if processed_preds.is_empty() { continue; }
            let new_dom = processed_preds.into_iter().reduce(|a, b| intersect_dom(&dom, a, b)).unwrap();
            if dom[v_node] != new_dom {
                dom[v_node] = new_dom;
                changed = true;
            }
        }
    }

    // Future: attach dom[] to ir for use by borrow/lifetime solver.
    let _ = dom;

    Ok(())
}

/// Intersect two dominator sets by walking the dominator tree.
fn intersect_dom(dom: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while a > b { a = dom[a]; }
        while b > a { b = dom[b]; }
    }
    a
}
