//! Trait Obligation Solver (S4) — method completeness.
//!
//! Variables:
//!   I  = { i | NodeKind::Impl { for_trait: Some(_) } }
//!   T  = { t | NodeKind::Trait { name } }
//!   M(t) = set of method names required by trait t
//!   C(i) = set of Method node names contained in impl i  (via G_module Contains)
//!
//! Equation:
//!   complete(i) <=> M(trait(i)) ⊆ C(i)
//!   missing(i)  = M(trait(i)) \ C(i)

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};
use std::collections::{HashMap, HashSet};
use crate::solver::csr_to_adj;

pub fn solve(ir: &ModelIR) -> Result<()> {
    let mod_v = ir.module_graph.vertex_count();
    if mod_v == 0 { return Ok(()); }

    // Build trait name -> required method names map
    // Equation: M(t) = { m.name | m ∈ trait.methods }
    let trait_methods: HashMap<&str, HashSet<&str>> = ir.nodes.iter().filter_map(|n| {
        if let NodeKind::Trait { name, methods, .. } = &n.kind {
            let ms: HashSet<&str> = methods.iter().map(|m| m.name.as_str()).collect();
            Some((name.as_str(), ms))
        } else { None }
    }).collect();

    // Build impl -> [child node indices] from module_graph Contains edges
    // Equation: C(i) = { j | (i, j, Contains) ∈ G_module }
    let adj = csr_to_adj(&ir.module_graph);
    let children_of = |idx: usize| -> Vec<usize> {
        if idx < adj.len() { adj[idx].clone() } else { vec![] }
    };

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let NodeKind::Impl { for_trait: Some(trait_name), .. } = &node.kind {
            let required = match trait_methods.get(trait_name.as_str()) {
                Some(m) => m,
                None => {
                    // Trait not in IR (external crate trait) — skip
                    continue;
                }
            };

            // Collect method names implemented
            let implemented: HashSet<&str> = children_of(idx).iter().filter_map(|&child| {
                match &ir.nodes.get(child)?.kind {
                    NodeKind::Method { name, .. } => Some(name.as_str()),
                    _ => None,
                }
            }).collect();

            let missing: Vec<&&str> = required.iter().filter(|m| !implemented.contains(**m)).collect();
            if !missing.is_empty() {
                eprintln!(
                    "WARN trait_solver: Impl[{}] for trait {:?} missing methods: {:?}",
                    idx, trait_name, missing
                );
            }
        }
    }

    Ok(())
}
