use crate::solver::csr_to_adj;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &CanonIR) -> Result<()> {
    let mod_v = ir.module_graph.vertex_count();
    if mod_v == 0 {
        return Ok(());
    }

    let trait_methods: HashMap<u32, HashSet<u32>> =
        ir.nodes.iter().filter_map(|n| if let CanonNodeKind::Trait { methods, .. } = &n.kind { Some((n.id.0, methods.iter().map(|m| m.0).collect())) } else { None }).collect();

    let adj = csr_to_adj(&ir.module_graph);
    let children_of = |idx: usize| -> Vec<usize> {
        if idx < adj.len() {
            adj[idx].clone()
        } else {
            vec![]
        }
    };

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let CanonNodeKind::Impl { for_trait: Some(trait_id), .. } = &node.kind {
            let required = match trait_methods.get(&trait_id.0) {
                Some(m) => m,
                None => continue,
            };

            let implemented: HashSet<u32> = children_of(idx)
                .iter()
                .filter_map(|&child| match &ir.nodes.get(child)?.kind {
                    CanonNodeKind::Fn { .. } => Some(child as u32),
                    _ => None,
                })
                .collect();

            let missing: Vec<u32> = required.iter().copied().filter(|m| !implemented.contains(m)).collect();
            if !missing.is_empty() {
                eprintln!("WARN trait_solver: Impl[{}] for trait {:?} missing methods {:?}", idx, trait_id, missing);
            }
        }
    }

    Ok(())
}
