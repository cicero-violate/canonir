use anyhow::Result;
use canon::edge::EdgeKind;
use canon::id::NodeId;
use canon::node::CanonNodeKind;
use canon::CanonIR;
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &CanonIR) -> Result<()> {
    let target_ids: HashSet<u32> = ir
        .nodes
        .iter()
        .filter_map(|n| match &n.kind {
            CanonNodeKind::Struct { .. } | CanonNodeKind::Enum { .. } | CanonNodeKind::Trait { .. } | CanonNodeKind::TypeAlias { .. } => Some(n.id.0),
            _ => None,
        })
        .collect();

    let mut impl_keys: HashMap<(u32, Option<u32>), Vec<usize>> = HashMap::new();

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let CanonNodeKind::Impl { for_ty, for_trait, .. } = &node.kind {
            if !target_ids.contains(&for_ty.0) {
                eprintln!("WARN impl_solver: Impl[{}] targets unknown type {:?}", idx, for_ty);
            }
            impl_keys.entry((for_ty.0, for_trait.map(|t| t.0))).or_default().push(idx);
        }
    }

    for ((for_ty, for_trait), indices) in &impl_keys {
        if indices.len() > 1 {
            eprintln!("WARN impl_solver: duplicate impl of {:?} for target {:?} at nodes {:?}", for_trait, for_ty, indices);
        }
    }

    let type_v = ir.type_graph.vertex_count();
    for (idx, node) in ir.nodes.iter().enumerate() {
        let CanonNodeKind::Impl { for_trait, .. } = &node.kind else {
            continue;
        };
        let mut implref_targets: Vec<u32> = Vec::new();
        if idx < type_v {
            for (dst_id, edge) in ir.type_graph.neighbours(NodeId(idx as u32)) {
                if *edge == EdgeKind::ImplRef {
                    implref_targets.push(dst_id.0);
                }
            }
        }
        implref_targets.sort_unstable();
        implref_targets.dedup();

        match (for_trait, implref_targets.as_slice()) {
            (Some(expected), [actual]) if expected.0 != *actual => {
                eprintln!("WARN impl_solver: Impl[{}] for_trait field {:?} mismatches type_graph ImplRef {:?}", idx, expected, actual);
            }
            (Some(_), []) => {
                eprintln!("WARN impl_solver: Impl[{}] missing ImplRef edge in type_graph", idx);
            }
            (None, [actual, ..]) => {
                eprintln!("WARN impl_solver: inherent Impl[{}] has unexpected ImplRef edge(s) {:?}", idx, implref_targets);
                let _ = actual;
            }
            (_, [_, _, ..]) => {
                eprintln!("WARN impl_solver: Impl[{}] has multiple ImplRef edges {:?}", idx, implref_targets);
            }
            _ => {}
        }
    }

    Ok(())
}
