//! GPU-accelerated SCC via forward/reverse reachability.
//! Uses GPU reachability kernel for each node (O(V*(V+E))).

use super::csr::Csr;
use super::reachability::reachability_batched_flat_gpu;

#[cfg(feature = "cuda")]
pub fn scc_gpu(csr: &Csr) -> Vec<Vec<usize>> {
    let v = csr.vertex_count();
    if v == 0 {
        return Vec::new();
    }
    let mut rev_adj: Vec<Vec<usize>> = vec![Vec::new(); v];
    for u in 0..v {
        let start = csr.row_ptr[u] as usize;
        let end = csr.row_ptr[u + 1] as usize;
        for &to in &csr.col_idx[start..end] {
            rev_adj[to as usize].push(u);
        }
    }
    let rev = Csr::from_adj(&rev_adj);
    let mut assigned = vec![false; v];
    let mut sccs = Vec::new();
    let roots: Vec<usize> = (0..v).collect();
    let fwd_flat = reachability_batched_flat_gpu(csr, &roots);
    let rev_flat = reachability_batched_flat_gpu(&rev, &roots);
    for i in 0..v {
        if assigned[i] {
            continue;
        }
        let base = i * v;
        let mut comp = Vec::new();
        for j in 0..v {
            if !assigned[j] && fwd_flat[base + j] != 0 && rev_flat[base + j] != 0 {
                assigned[j] = true;
                comp.push(j);
            }
        }
        if !comp.is_empty() {
            sccs.push(comp);
        }
    }
    sccs
}
