//! Call-graph solver — reachability from crate roots.
//!
//! Variables:
//!   G_call     = ir.call_graph        (CSR)
//!   roots      = [v | node[v] is a root function (no incoming call edges)]
//!   reachable  = union of dfs(root) for root in roots
//!   dead       = {v in V} \ reachable
//!
//! Equations:
//!   in_degree[v] = |{ u | (u,v) in G_call }|
//!   roots        = { v | in_degree[v] = 0 }
//!   reachable    = DFS(G_call, roots)
//!
//! Algorithm used: DFS (from algorithms crate)

use anyhow::Result;
use model::ir::model_ir::ModelIR;
use algorithms::graph::dfs::dfs;
use crate::solver::csr_to_adj;

pub fn solve(ir: &mut ModelIR) -> Result<()> {
    let v = ir.call_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj = csr_to_adj(&ir.call_graph);

    // Compute in-degrees to find roots.
    let mut in_degree = vec![0usize; v];
    for neighbours in &adj {
        for &dst in neighbours {
            in_degree[dst] += 1;
        }
    }

    // DFS from every root; union into reachability set.
    let mut reachable = vec![false; v];
    for root in 0..v {
        if in_degree[root] == 0 {
            for idx in dfs(&adj, root) {
                reachable[idx] = true;
            }
        }
    }

    // Dead nodes: present in call_graph but never reached.
    // Future: surface as lint warnings via diagnostics channel.
    let _dead: Vec<usize> = (0..v).filter(|&i| !reachable[i]).collect();

    Ok(())
}
