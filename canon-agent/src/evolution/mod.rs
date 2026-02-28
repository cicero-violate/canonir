pub mod lyapunov;
mod structural;
pub use lyapunov::{
    enforce_lyapunov_bound, StructureDriftError, StructureMetrics, DEFAULT_TOPOLOGY_THETA,
};
use crate::ir::{SystemState, DeltaId, CodeDelta};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvolutionError {
    #[error("struct `{0}` does not exist")]
    UnknownStruct(String),
    #[error("function `{0}` does not exist")]
    UnknownFunction(String),
    #[error("trait `{0}` does not exist")]
    UnknownTrait(String),
    #[error("trait function `{0}` does not exist")]
    UnknownTraitFunction(String),
    #[error("module `{0}` does not exist")]
    UnknownModule(String),
    #[error("impl `{0}` does not exist")]
    UnknownImpl(String),
    #[error("execution `{0}` does not exist")]
    UnknownExecution(String),
    #[error("enum `{0}` does not exist")]
    UnknownEnum(String),
    #[error("unknown delta `{0}`")]
    UnknownDelta(DeltaId),
    #[error("artifact `{0}` already exists")]
    DuplicateArtifact(String),
    #[error("field `{field}` does not exist on struct `{struct_id}`")]
    UnknownField { struct_id: String, field: String },
    #[error("topology drift rejected: {0}")]
    TopologyDrift(StructureDriftError),
}

pub fn apply_admitted_deltas(
    ir: &SystemState,
    _admission_ids: &[String],
) -> Result<(SystemState, Vec<CodeDelta>), EvolutionError> {
    // TODO: real structural delta application + diff → CodeDelta
    let next = ir.clone();
    let code_deltas = Vec::new();
    Ok((next, code_deltas))
}
