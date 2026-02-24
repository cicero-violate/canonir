//! Impl Resolution Solver (S3).
//!
//! Variables:
//!   I  = { i ∈ V | NodeKind::Impl }
//!   ST = { name | NodeKind::Struct { name } ∈ V }
//!   TR = { name | NodeKind::Trait  { name } ∈ V }
//!
//! Equations:
//!   valid(i)      <=> i.for_struct ∈ ST
//!   duplicate(i,j)<=> i≠j ∧ i.for_struct=j.for_struct ∧ i.for_trait=j.for_trait
//!   orphan(i)     <=> i.for_trait.is_some()
//!                     ∧ i.for_struct ∉ ST   (external type — simplification)

use anyhow::Result;
use model::ir::{model_ir::ModelIR, node::NodeKind};
use std::collections::{HashMap, HashSet};

pub fn solve(ir: &ModelIR) -> Result<()> {
    let struct_names: HashSet<&str> = ir.nodes.iter().filter_map(|n| {
        if let NodeKind::Struct { name, .. } = &n.kind { Some(name.as_str()) } else { None }
    }).collect();

    // Collect impls keyed by (for_struct, for_trait)
    // Equation: duplicate(i,j) detected as count > 1 per key
    let mut impl_keys: HashMap<(String, Option<String>), Vec<usize>> = HashMap::new();

    for (idx, node) in ir.nodes.iter().enumerate() {
        if let NodeKind::Impl { for_struct, for_trait, .. } = &node.kind {
            // valid_impl check
            if !struct_names.contains(for_struct.as_str()) {
                eprintln!(
                    "WARN impl_solver: Impl[{}] targets unknown struct {:?}",
                    idx, for_struct
                );
            }
            impl_keys
                .entry((for_struct.clone(), for_trait.clone()))
                .or_default()
                .push(idx);
        }
    }

    // Duplicate impl detection
    for ((for_struct, for_trait), indices) in &impl_keys {
        if indices.len() > 1 {
            eprintln!(
                "WARN impl_solver: duplicate impl of {:?} for struct {:?} at nodes {:?}",
                for_trait, for_struct, indices
            );
        }
    }

    Ok(())
}
