use anyhow::Result;
use canon::id::NodeId;
use canon::node::{flags, CanonNodeKind};
use canon::CanonIR;

pub fn solve(ir: &CanonIR) -> Result<()> {
    if ir.call_graph.vertex_count() == 0 {
        return Ok(());
    }

    let unsafe_set: Vec<bool> = ir
        .nodes
        .iter()
        .map(|n| match &n.kind {
            CanonNodeKind::Fn { flags: f, .. } | CanonNodeKind::Impl { flags: f, .. } | CanonNodeKind::Trait { flags: f, .. } => (*f & flags::UNSAFE) != 0,
            _ => false,
        })
        .collect();

    let v = ir.call_graph.vertex_count().min(ir.nodes.len());

    for caller_idx in 0..v {
        let caller_id = NodeId(caller_idx as u32);
        let caller_unsafe = unsafe_set.get(caller_idx).copied().unwrap_or(false);
        for (callee_id, _) in ir.call_graph.neighbours(caller_id) {
            let callee_unsafe = unsafe_set.get(callee_id.index()).copied().unwrap_or(false);
            if callee_unsafe && !caller_unsafe {
                let caller_name = node_name(ir, caller_idx);
                let callee_name = node_name(ir, callee_id.index());
                log::warn!("unsafe_solver: safe fn `{}` calls unsafe fn `{}` without unsafe block", caller_name, callee_name);
            }
        }
    }

    Ok(())
}

fn node_name(ir: &CanonIR, idx: usize) -> String {
    ir.nodes
        .get(idx)
        .map(|n| match &n.kind {
            CanonNodeKind::Fn { name_id, .. } => ir.lookup_name(*name_id).to_string(),
            _ => format!("node_{}", idx),
        })
        .unwrap_or_else(|| format!("node_{}", idx))
}
