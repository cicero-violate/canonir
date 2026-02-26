use crate::solver::csr_to_adj;
use algorithms::graph::scc::kosaraju_scc;
use anyhow::Result;
use canon::node::{CanonId, CanonNodeKind, TypeKind};
use canon::CanonIR;

fn node_ty(ir: &CanonIR, idx: usize) -> Option<CanonId> {
    match &ir.nodes.get(idx)?.kind {
        CanonNodeKind::Fn { sig_id, .. } => match &ir.node(*sig_id).kind {
            CanonNodeKind::FnSig { ret, .. } => Some(*ret),
            _ => None,
        },
        CanonNodeKind::TypeAlias { ty, .. } => Some(*ty),
        CanonNodeKind::TypeRef { .. } => Some(canon::node::CanonId(idx as u32)),
        _ => None,
    }
}

fn is_concrete(ir: &CanonIR, ty: CanonId) -> bool {
    match &ir.node(ty).kind {
        CanonNodeKind::Type { kind: TypeKind::Param(_) } => false,
        CanonNodeKind::TypeRef { name_id } => {
            let s = ir.lookup_name(*name_id);
            !(s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase()))
        }
        _ => true,
    }
}

pub fn solve(ir: &CanonIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 {
        return Ok(());
    }

    let adj = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let tys: Vec<(usize, CanonId)> = scc.iter().filter_map(|&idx| node_ty(ir, idx).map(|t| (idx, t))).collect();

        let concrete: Vec<(usize, CanonId)> = tys.iter().copied().filter(|(_, t)| is_concrete(ir, *t)).collect();

        if concrete.len() > 1 {
            let first_ty = concrete[0].1;
            for &(idx, ty) in &concrete[1..] {
                if ty != first_ty {
                    eprintln!("WARN generic_solver: type conflict in SCC: node {} has {:?} vs {:?}", idx, ty, first_ty);
                }
            }
        }
    }

    Ok(())
}
