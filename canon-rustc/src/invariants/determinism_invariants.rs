use anyhow::{bail, Result};
use canon_ir::ir::CanonIR;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn validate_emit_order(ir: &CanonIR) -> Result<()> {
    for (idx, node) in ir.nodes.iter().enumerate() {
        if node.id.0 != idx as u32 {
            bail!(
                "Invariant violation: canon node order mismatch idx={} node_id={}",
                idx,
                node.id.0
            );
        }
    }
    let mut last: Option<u32> = None;
    for id in &ir.emit_order {
        if id.0 as usize >= ir.nodes.len() {
            bail!(
                "Invariant violation: emit_order contains out-of-range node id={}",
                id.0
            );
        }
        if let Some(prev) = last {
            if id.0 < prev {
                bail!(
                    "Invariant violation: emit_order not sorted prev={} curr={}",
                    prev,
                    id.0
                );
            }
        }
        last = Some(id.0);
    }
    Ok(())
}

pub fn compute_invariant_hash(node_count: u64, edge_count: u64, schema_version: u64) -> String {
    let mut hasher = DefaultHasher::new();
    node_count.hash(&mut hasher);
    edge_count.hash(&mut hasher);
    schema_version.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
