use crate::solver::csr_to_adj;
use algorithms::graph::dfs::dfs;
use anyhow::Result;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let v = ir.call_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.call_graph);
    let mut in_degree = vec![0usize; v];
    for neighbours in &adj {
        for &dst in neighbours {
            in_degree[dst] += 1;
        }
    }

    let mut reachable = vec![false; v];
    for root in 0..v {
        if in_degree[root] == 0 {
            for idx in dfs(&adj, root) {
                reachable[idx] = true;
            }
        }
    }

    let _dead: Vec<usize> = (0..v).filter(|&i| !reachable[i]).collect();
    Ok(())
}
