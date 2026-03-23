use anyhow::Result;
use canon_ir::ir::CanonIR;

pub mod csr_invariants;
pub mod determinism_invariants;
pub mod edge_invariants;
pub mod node_invariants;

pub fn validate_structural(ir: &CanonIR) -> Result<()> {
    node_invariants::validate_unique_ids(ir)?;
    edge_invariants::validate_edge_endpoints(ir)?;
    csr_invariants::validate_csr(ir)?;
    determinism_invariants::validate_emit_order(ir)?;
    Ok(())
}

pub fn compute_invariant_hash(node_count: u64, edge_count: u64, schema_version: u64) -> String {
    determinism_invariants::compute_invariant_hash(node_count, edge_count, schema_version)
}
