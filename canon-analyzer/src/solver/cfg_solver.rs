use crate::solver::csr_to_adj;
use algorithms::graph::dfs::dfs;
use anyhow::Result;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.cfg_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.cfg_graph);
    let reached = dfs(&adj, 0);
    let reachable: std::collections::HashSet<usize> = reached.into_iter().collect();
    let _dead: Vec<usize> = (0..v).filter(|i| !reachable.contains(i)).collect();

    let undef = usize::MAX;
    let mut dom = vec![undef; v];
    dom[0] = 0;

    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); v];
    for (u, nbrs) in adj.iter().enumerate() {
        for &w in nbrs {
            pred[w].push(u);
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for v_node in 1..v {
            if !reachable.contains(&v_node) {
                continue;
            }
            let processed_preds: Vec<usize> = pred[v_node].iter().filter(|&&p| dom[p] != undef).copied().collect();
            if processed_preds.is_empty() {
                continue;
            }
            let new_dom = processed_preds.into_iter().reduce(|a, b| intersect_dom(&dom, a, b)).unwrap();
            if dom[v_node] != new_dom {
                dom[v_node] = new_dom;
                changed = true;
            }
        }
    }

    let _ = dom;
    Ok(())
}

fn intersect_dom(dom: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while a > b {
            a = dom[a];
        }
        while b > a {
            b = dom[b];
        }
    }
    a
}
