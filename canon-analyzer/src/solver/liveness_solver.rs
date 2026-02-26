use crate::solver::csr_to_adj;
use algorithms::graph::reachability::reachability;
use anyhow::Result;
use canon::node::{flags, CanonNodeKind};
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let call_v = ir.call_graph.vertex_count();
    if call_v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.call_graph);

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

    let live = reachability(&adj, &roots);

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
