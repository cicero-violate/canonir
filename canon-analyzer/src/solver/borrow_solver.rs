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
