use crate::solver::csr_to_adj;
use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
use canon::node::CanonNodeKind;
use canon::CanonIR;
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &CanonIR) -> Result<()> {
    let mod_v = ir.module_graph.vertex_count();
    if mod_v == 0 {
        return Ok(());
    }

    let trait_methods: HashMap<u32, HashSet<String>> = ir
        .nodes
        .iter()
        .filter_map(|n| {
            if let CanonNodeKind::Trait { methods, .. } = &n.kind {
                let names: HashSet<String> = methods
                    .iter()
                    .filter_map(|m| match &ir.node(*m).kind {
                        CanonNodeKind::Fn { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
                        _ => None,
                    })
                    .collect();
                Some((n.id.0, names))
            } else {
                None
            }
        })
        .collect();

    let adj = csr_to_adj(&ir.module_graph);
    let children_of = |idx: usize| -> Vec<usize> {
        if idx < adj.len() {
            adj[idx].clone()
        } else {
            vec![]
        }
    };
    let type_v = ir.type_graph.vertex_count();
    let mut impl_trait_from_graph: HashMap<usize, Vec<u32>> = HashMap::new();
    for src_idx in 0..type_v {
        for (dst_id, edge) in ir.type_graph.neighbours(NodeId(src_idx as u32)) {
            if *edge == EdgeKind::ImplRef {
                impl_trait_from_graph.entry(src_idx).or_default().push(dst_id.0);
            }
        }
    }
    for traits in impl_trait_from_graph.values_mut() {
        traits.sort_unstable();
        traits.dedup();
    }

    for (idx, node) in ir.nodes.iter().enumerate() {
        let CanonNodeKind::Impl { for_trait, .. } = &node.kind else {
            continue;
        };
        let trait_id = match impl_trait_from_graph.get(&idx).and_then(|ids| ids.first()).copied() {
            Some(id) => id,
            None => match for_trait {
                Some(id) => id.0,
                None => continue,
            },
        };
        if impl_trait_from_graph.get(&idx).is_none() && for_trait.is_some() {
            eprintln!("WARN trait_solver: Impl[{}] has for_trait but no ImplRef edge in type_graph", idx);
        }
        if let Some(ids) = impl_trait_from_graph.get(&idx) {
            if ids.len() > 1 {
                eprintln!("WARN trait_solver: Impl[{}] has multiple ImplRef traits {:?}", idx, ids);
            }
            if let Some(field_trait) = for_trait {
                if !ids.contains(&field_trait.0) {
                    eprintln!(
                        "WARN trait_solver: Impl[{}] for_trait {:?} mismatches type_graph {:?}",
                        idx, field_trait, ids
                    );
                }
            }
        }

        let required = match trait_methods.get(&trait_id) {
            Some(m) => m,
            None => continue,
        };

        let implemented: HashSet<String> = children_of(idx)
            .iter()
            .filter_map(|&child| match &ir.nodes.get(child)?.kind {
                CanonNodeKind::Fn { name_id, .. } => Some(ir.lookup_name(*name_id).to_string()),
                _ => None,
            })
            .collect();

        let missing: Vec<String> = required.iter().filter(|m| !implemented.contains(*m)).cloned().collect();
        if !missing.is_empty() {
            eprintln!("WARN trait_solver: Impl[{}] for trait {:?} missing methods {:?}", idx, trait_id, missing);
        }
    }

    Ok(())
}
