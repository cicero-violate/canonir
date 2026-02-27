use crate::solver::csr_to_adj;
use algorithms::graph::reachability::reachability;
use anyhow::Result;
use canon::node::{flags, CanonNodeKind};
use canon::CanonIR;
use canon::{edge::EdgeKind, id::NodeId};

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let n = ir.nodes.len();
    let name_v = ir.name_graph.vertex_count();
    let mod_v = ir.module_graph.vertex_count();
    if name_v == 0 || mod_v == 0 {
        return Ok(());
    }

    let fwd = csr_to_adj(&ir.module_graph);
    let mut inv: Vec<Vec<usize>> = vec![Vec::new(); mod_v.max(n)];
    for (src, nbrs) in fwd.iter().enumerate() {
        for &dst in nbrs {
            if dst < inv.len() {
                inv[dst].push(src);
            }
        }
    }

    let containing_module = |start: usize| -> Option<usize> {
        if start >= inv.len() {
            return None;
        }
        let mut stack = vec![start];
        let mut seen = vec![false; inv.len()];
        while let Some(u) = stack.pop() {
            if seen[u] {
                continue;
            }
            seen[u] = true;
            if let Some(CanonNodeKind::Module { .. }) = ir.nodes.get(u).map(|n| &n.kind) {
                return Some(u);
            }
            for &p in &inv[u] {
                if !seen[p] {
                    stack.push(p);
                }
            }
        }
        None
    };

    let ancestor_or_eq = |a: usize, b: usize| -> bool {
        if a == b {
            return true;
        }
        if a >= fwd.len() {
            return false;
        }
        let reach = reachability(&fwd, &[a]);
        b < reach.len() && reach[b]
    };

    let parent_module = |m: usize| -> Option<usize> { inv.get(m)?.first().copied() };

    let mut warnings: Vec<String> = Vec::new();

    for src_idx in 0..name_v {
        let src_id = NodeId(src_idx as u32);
        for (dst_id, edge) in ir.name_graph.neighbours(src_id) {
            if *edge != EdgeKind::Resolves {
                continue;
            }
            let dst_idx = dst_id.index();
            let vis_flags = visibility_flags(ir.nodes.get(dst_idx).map(|n| &n.kind));

            let ok = if vis_flags & flags::PUB != 0 || vis_flags & flags::PUB_CRATE != 0 {
                true
            } else if vis_flags & flags::PUB_SUPER != 0 {
                if let (Some(sm), Some(dm)) = (containing_module(src_idx), containing_module(dst_idx)) {
                    parent_module(dm).map(|p| ancestor_or_eq(p, sm)).unwrap_or(false)
                } else {
                    false
                }
            } else {
                containing_module(src_idx) == containing_module(dst_idx)
            };

            if !ok {
                warnings.push(format!("visibility_solver: node {} accesses private item {}", src_idx, dst_idx));
            }
        }
    }

    for w in &warnings {
        eprintln!("WARN {}", w);
    }

    Ok(())
}

fn visibility_flags(kind: Option<&CanonNodeKind>) -> u32 {
    match kind {
        Some(CanonNodeKind::Module { flags, .. }) => *flags,
        Some(CanonNodeKind::Struct { flags, .. }) => *flags,
        Some(CanonNodeKind::Enum { flags, .. }) => *flags,
        Some(CanonNodeKind::Trait { flags, .. }) => *flags,
        Some(CanonNodeKind::Fn { flags, .. }) => *flags,
        Some(CanonNodeKind::Field { flags, .. }) => *flags,
        Some(CanonNodeKind::Param { flags, .. }) => *flags,
        Some(CanonNodeKind::Const { flags, .. }) => *flags,
        Some(CanonNodeKind::Static { flags, .. }) => *flags,
        Some(CanonNodeKind::Use { flags, .. }) => *flags,
        Some(CanonNodeKind::ExternCrate { flags, .. }) => *flags,
        Some(CanonNodeKind::TypeAlias { flags, .. }) => *flags,
        Some(CanonNodeKind::Impl { flags, .. }) => *flags,
        _ => 0,
    }
}
