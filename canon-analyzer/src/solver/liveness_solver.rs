#[cfg(not(feature = "cuda"))]
use crate::solver::csr_to_adj;
#[cfg(feature = "cuda")]
use crate::solver::gpu_algorithms::reachability_gpu;
#[cfg(feature = "cuda")]
use crate::solver::graph_to_csr;
use anyhow::Result;
use canon::node::{flags, CanonNodeKind};
use canon::CanonIR;
#[cfg(not(feature = "cuda"))]
use std::collections::VecDeque;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let call_v = ir.call_graph.vertex_count();
    if call_v == 0 {
        return Ok(());
    }

    let roots: Vec<usize> = ir
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, n)| match &n.kind {
            CanonNodeKind::Fn { name_id, flags: f, .. } => {
                let name = ir.lookup_name(*name_id);
                if name == "main" || (*f & flags::PUB) != 0 {
                    Some(idx)
                } else {
                    None
                }
            }
            CanonNodeKind::Crate { .. } => Some(idx),
            _ => None,
        })
        .filter(|&idx| idx < call_v)
        .collect();

    if roots.is_empty() {
        return Ok(());
    }

    let live = {
        #[cfg(feature = "cuda")]
        {
            let csr = graph_to_csr(&ir.call_graph);
            reachability_gpu(&csr, &roots)
        }
        #[cfg(not(feature = "cuda"))]
        {
            let adj = csr_to_adj(&ir.call_graph);
            reachability_mask(&adj, &roots)
        }
    };

    let before = ir.emit_order.len();
    ir.emit_order.retain(|&id| {
        let idx = id.0 as usize;
        match ir.nodes.get(idx).map(|n| &n.kind) {
            Some(CanonNodeKind::Fn { flags: f, .. }) => {
                if idx < live.len() && live[idx] {
                    return true;
                }
                (*f & flags::PUB) != 0
            }
            _ => true,
        }
    });
    let removed = before - ir.emit_order.len();
    if removed > 0 {
        eprintln!("INFO liveness_solver: pruned {} dead function(s) from emit_order", removed);
    }

    Ok(())
}

#[cfg(not(feature = "cuda"))]
fn reachability_mask(adj: &[Vec<usize>], roots: &[usize]) -> Vec<bool> {
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut q = VecDeque::new();
    for &r in roots {
        if r < n && !visited[r] {
            visited[r] = true;
            q.push_back(r);
        }
    }
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                q.push_back(v);
            }
        }
    }
    visited
}
