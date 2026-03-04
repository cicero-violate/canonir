#[cfg(feature = "cuda")]
use crate::solver::graph_to_csr;
#[cfg(feature = "cuda")]
use crate::solver::gpu_algorithms::reachability_gpu;
use algorithms::graph::region::outlives_cycles;
use anyhow::{bail, Result};
use canon::id::NodeId;
use canon::node::CanonNodeKind;
use canon::CanonIR;

pub fn solve(ir: &CanonIR) -> Result<()> {
    let v = ir.region_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    #[cfg(feature = "cuda")]
    {
        return solve_with_gpu(ir, v);
    }
    #[cfg(not(feature = "cuda"))]
    {
        return solve_cpu(ir, v);
    }
}

#[cfg(not(feature = "cuda"))]
fn solve_cpu(ir: &CanonIR, v: usize) -> Result<()> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); v];
    for (idx, node) in ir.nodes.iter().enumerate() {
        if idx >= v {
            break;
        }
        if !matches!(&node.kind, CanonNodeKind::Lifetime { .. }) {
            continue;
        }
        for (nb, _edge) in ir.region_graph.neighbours(NodeId(node.id.0)) {
            adj[idx].push(nb.index());
        }
    }

    let cycles = outlives_cycles(&adj);
    if cycles.is_empty() {
        log::info!("borrow_solver: region graph is acyclic - {} lifetime node(s) valid", v);
        return Ok(());
    }

    let names: Vec<String> = cycles
        .iter()
        .map(|scc| {
            scc.iter()
                .filter_map(|&i| ir.nodes.get(i).and_then(|n| if let CanonNodeKind::Lifetime { name_id } = &n.kind { Some(ir.lookup_name(*name_id).to_string()) } else { None }))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();

    bail!("borrow_solver: conflicting lifetime constraints - cycles: [{}]", names.join("; "));
}

#[cfg(feature = "cuda")]
fn solve_with_gpu(ir: &CanonIR, v: usize) -> Result<()> {
    let mut is_lifetime = vec![false; v];
    for idx in 0..v {
        if matches!(ir.nodes.get(idx).map(|n| &n.kind), Some(CanonNodeKind::Lifetime { .. })) {
            is_lifetime[idx] = true;
        }
    }

    let mut indegree = vec![0usize; v];
    for src in 0..v {
        if !is_lifetime[src] {
            continue;
        }
        for (dst, _edge) in ir.region_graph.neighbours(NodeId(src as u32)) {
            let dst_idx = dst.index();
            if dst_idx < v && is_lifetime[dst_idx] {
                indegree[dst_idx] += 1;
            }
        }
    }

    let mut roots: Vec<usize> = (0..v).filter(|&i| is_lifetime[i] && indegree[i] == 0).collect();
    if roots.is_empty() {
        roots = (0..v).filter(|&i| is_lifetime[i]).collect();
    }
    if roots.is_empty() {
        return Ok(());
    }

    let csr = graph_to_csr(&ir.region_graph);
    let reachable = reachability_gpu(&csr, &roots);

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); v];
    for idx in 0..v {
        if !is_lifetime[idx] {
            continue;
        }
        if !reachable.get(idx).copied().unwrap_or(false) {
            continue;
        }
        for (nb, _edge) in ir.region_graph.neighbours(NodeId(idx as u32)) {
            let nb_idx = nb.index();
            if nb_idx < v && is_lifetime[nb_idx] && reachable.get(nb_idx).copied().unwrap_or(false) {
                adj[idx].push(nb_idx);
            }
        }
    }

    let cycles = outlives_cycles(&adj);
    if cycles.is_empty() {
        log::info!("borrow_solver: region graph is acyclic - {} lifetime node(s) valid", v);
        return Ok(());
    }

    let names: Vec<String> = cycles
        .iter()
        .map(|scc| {
            scc.iter()
                .filter_map(|&i| ir.nodes.get(i).and_then(|n| if let CanonNodeKind::Lifetime { name_id } = &n.kind { Some(ir.lookup_name(*name_id).to_string()) } else { None }))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();

    bail!("borrow_solver: conflicting lifetime constraints - cycles: [{}]", names.join("; "));
}
