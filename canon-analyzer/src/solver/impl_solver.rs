use anyhow::Result;
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

    Ok(())
}
