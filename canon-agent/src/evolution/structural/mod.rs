mod apply;
mod guard;
mod rename;
use super::EvolutionError;
use crate::ir::{SystemState, StateChange};
pub(super) fn apply_structural_delta(
    ir: &mut SystemState,
    delta: &StateChange,
) -> Result<(), EvolutionError> {
    if let Some(payload) = &delta.payload {
        apply::apply_delta_payload(ir, payload, &delta.id)?;
    }
    Ok(())
}
