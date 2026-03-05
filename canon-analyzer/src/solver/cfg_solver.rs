use crate::solver::csr_to_adj;
#[cfg(not(feature = "cuda"))]
use crate::solver::gpu_algorithms::dfs;
use crate::solver::gpu_algorithms::dominators;
#[cfg(feature = "cuda")]
use crate::solver::gpu_algorithms::reachability_gpu;
#[cfg(feature = "cuda")]
use crate::solver::graph_to_csr;
use anyhow::Result;
use canon::CanonIR;
use std::collections::HashMap;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.cfg_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.cfg_graph);
    let reachable: Vec<bool> = {
        #[cfg(feature = "cuda")]
        {
            let csr = graph_to_csr(&ir.cfg_graph);
            reachability_gpu(&csr, &[0])
        }
        #[cfg(not(feature = "cuda"))]
        {
            let reached = dfs(&adj, 0);
            let mut mask = vec![false; v];
            for idx in reached {
                if idx < v {
                    mask[idx] = true;
                }
            }
            mask
        }
    };
    let _dead: Vec<usize> = reachable.iter().enumerate().filter_map(|(i, &r)| if r { None } else { Some(i) }).collect();

    let mut preds: HashMap<usize, Vec<usize>> = HashMap::new();
    for (u, nbrs) in adj.iter().enumerate() {
        for &w in nbrs {
            preds.entry(w).or_default().push(u);
        }
    }
    let _dom = dominators(v, &preds, 0);

    Ok(())
}
