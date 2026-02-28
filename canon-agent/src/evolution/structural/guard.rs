use super::super::EvolutionError;
use crate::ir::SystemState;
pub fn ensure_module_exists(
    ir: &SystemState,
    module: &str,
) -> Result<(), EvolutionError> {
    if ir.modules.iter().any(|m| m.id == module) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownModule(module.to_string()))
    }
}
pub fn ensure_struct_exists(
    ir: &SystemState,
    struct_id: &str,
) -> Result<(), EvolutionError> {
    if ir.structs.iter().any(|s| s.id == struct_id) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownStruct(struct_id.to_string()))
    }
}
pub fn ensure_field_exists(
    ir: &SystemState,
    struct_id: &str,
    field_name: &str,
) -> Result<(), EvolutionError> {
    let structure = ir
        .structs
        .iter()
        .find(|s| s.id == struct_id)
        .ok_or_else(|| EvolutionError::UnknownStruct(struct_id.to_string()))?;
    if structure.fields.iter().any(|f| f.name.as_str() == field_name) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownField {
            struct_id: struct_id.to_string(),
            field: field_name.to_string(),
        })
    }
}
pub fn ensure_trait_exists(
    ir: &SystemState,
    trait_id: &str,
) -> Result<(), EvolutionError> {
    if ir.traits.iter().any(|t| t.id == trait_id) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownTrait(trait_id.to_string()))
    }
}
pub fn ensure_trait_function_exists(
    ir: &SystemState,
    trait_id: &str,
    trait_fn: &str,
) -> Result<(), EvolutionError> {
    let tr = ir
        .traits
        .iter()
        .find(|t| t.id == trait_id)
        .ok_or_else(|| EvolutionError::UnknownTrait(trait_id.to_string()))?;
    if tr.functions.iter().any(|f| f.id == trait_fn) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownTraitFunction(trait_fn.to_string()))
    }
}
pub fn ensure_function_exists(
    ir: &SystemState,
    function_id: &str,
) -> Result<(), EvolutionError> {
    if ir.functions.iter().any(|f| f.id == function_id) {
        Ok(())
    } else {
        Err(EvolutionError::UnknownFunction(function_id.to_string()))
    }
}
