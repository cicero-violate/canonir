//! GPU-accelerated SCC via forward/reverse reachability.
//! Uses GPU reachability kernel for each node (O(V*(V+E))).

use super::csr::Csr;
use super::reachability::reachability_gpu;

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
    for i in 0..v {
        if assigned[i] {
            continue;
        }
        let fwd = reachability_gpu(csr, &[i]);
        let rev = reachability_gpu(&rev, &[i]);
        let mut comp = Vec::new();
        for j in 0..v {
            if !assigned[j] && fwd[j] && rev[j] {
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
