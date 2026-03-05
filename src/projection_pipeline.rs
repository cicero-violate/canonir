use crate::projection_helpers::{plan_emission, validate_structure, normalize_modules, ItemNode};

/// Entry point used by canon-projection before writing emitted Rust sources.
/// Ensures module layout normalization, dependency ordered emission and
/// structural validation of the projected IR.
pub fn emit_pipeline(items: Vec<ItemNode>) -> Result<Vec<ItemNode>, String> {
    // 1. Validate structural integrity
    let errors = validate_structure(&items);
    if !errors.is_empty() {
        return Err(format!("structural validation failed: {:?}", errors));
    }

    // 2. Normalize module grouping
    let _modules = normalize_modules(&items);

    // 3. Compute deterministic dependency‑ordered emission plan
    let ordered = plan_emission(&items)?;

    Ok(ordered)
}

/// Helper used by the projection stage to guard file emission.
pub fn prepare_emission(items: Vec<ItemNode>) -> Result<Vec<ItemNode>, String> {
    emit_pipeline(items)
}
