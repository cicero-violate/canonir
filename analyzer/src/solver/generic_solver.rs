//! Generic Constraint Solver (S5) — TypeUnifies propagation.
//!
//! Variables:
//!   G_type = ir.type_graph   (TypeOf, TypeUnifies edges)
//!   ty(v)  = type string of node v (ret or field ty)
//!   sccs   = kosaraju_scc(G_type)
//!
//! Equations:
//!   TypeUnifies(u, v) => ty(u) ≡ ty(v)
//!   conflict(u, v)    <=> TypeUnifies(u,v) ∧ ty(u) ≠ ty(v)
//!                         ∧ ¬contains("T", ty(u)) ∧ ¬contains("T", ty(v))
//!
//! Algorithm: Kosaraju SCC to find unification groups, then check concrete conflicts.

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};
use algorithms::graph::scc::kosaraju_scc;
use crate::solver::csr_to_adj;

/// Extract a representative type string from a node (best-effort).
fn node_ty(kind: &NodeKind) -> Option<&str> {
    match kind {
        NodeKind::Function { ret, .. } => Some(ret.as_str()),
        NodeKind::Method   { ret, .. } => Some(ret.as_str()),
        NodeKind::TypeAlias { ty, .. }  => Some(ty.as_str()),
        NodeKind::TypeRef   { name }    => Some(name.as_str()),
        _ => None,
    }
}

/// A type string is "concrete" if it contains no single-letter uppercase placeholder.
fn is_concrete(ty: &str) -> bool {
    !ty.chars().any(|c| c.is_uppercase() && ty.len() == 1)
}

pub fn solve(ir: &ModelIR) -> Result<()> {
    let v = ir.type_graph.vertex_count();
    if v == 0 { return Ok(()); }

    let adj  = csr_to_adj(&ir.type_graph);
    let sccs = kosaraju_scc(&adj);

    // For each SCC with >1 node, check for concrete type conflicts.
    // Equation: conflict(u,v) <=> TypeUnifies(u,v) ∧ ty(u)≠ty(v) ∧ concrete(u) ∧ concrete(v)
    for scc in sccs.iter().filter(|s| s.len() > 1) {
        let tys: Vec<(usize, &str)> = scc.iter().filter_map(|&idx| {
            ir.nodes.get(idx).and_then(|n| node_ty(&n.kind).map(|t| (idx, t)))
        }).collect();

        let concrete: Vec<(usize, &str)> = tys.iter()
            .filter(|(_, t)| is_concrete(t))
            .copied()
            .collect();

        if concrete.len() > 1 {
            let first_ty = concrete[0].1;
            for &(idx, ty) in &concrete[1..] {
                if ty != first_ty {
                    eprintln!(
                        "WARN generic_solver: type conflict in SCC: node {} has {:?} vs {:?}",
                        idx, ty, first_ty
                    );
                }
            }
        }
    }

    Ok(())
}
