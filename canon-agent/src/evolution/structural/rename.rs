use super::super::EvolutionError;
use crate::ir::SystemState;
pub fn rename_module(
    ir: &mut SystemState,
    old_id: &str,
    new_id: &str,
) -> Result<(), EvolutionError> {
    let module = ir
        .modules
        .iter_mut()
        .find(|m| m.id == *old_id)
        .ok_or_else(|| EvolutionError::UnknownModule(old_id.to_string()))?;
    module.id = new_id.to_owned();
    for structure in &mut ir.structs {
        if structure.module == *old_id {
            structure.module = new_id.to_owned();
        }
    }
    for tr in &mut ir.traits {
        if tr.module == *old_id {
            tr.module = new_id.to_owned();
        }
    }
    for block in &mut ir.impls {
        if block.module == *old_id {
            block.module = new_id.to_owned();
        }
    }
    for function in &mut ir.functions {
        if function.module == *old_id {
            function.module = new_id.to_owned();
        }
    }
    for edge in &mut ir.module_edges {
        if edge.source == *old_id {
            edge.source = new_id.to_owned();
        }
        if edge.target == *old_id {
            edge.target = new_id.to_owned();
        }
    }
    Ok(())
}
pub fn rename_struct(
    ir: &mut SystemState,
    old_id: &str,
    new_id: &str,
) -> Result<(), EvolutionError> {
    let structure = ir
        .structs
        .iter_mut()
        .find(|s| s.id == *old_id)
        .ok_or_else(|| EvolutionError::UnknownStruct(old_id.to_string()))?;
    structure.id = new_id.to_owned();
    for block in &mut ir.impls {
        if block.struct_id == *old_id {
            block.struct_id = new_id.to_owned();
        }
    }
    for function in &mut ir.functions {
        if function.module == *old_id {
            function.module = new_id.to_owned();
        }
    }
    Ok(())
}
pub fn rename_function(
    ir: &mut SystemState,
    old_id: &str,
    new_id: &str,
) -> Result<(), EvolutionError> {
    let function = ir
        .functions
        .iter_mut()
        .find(|f| f.id == *old_id)
        .ok_or_else(|| EvolutionError::UnknownFunction(old_id.to_string()))?;
    function.id = new_id.to_owned();
    for block in &mut ir.impls {
        for binding in &mut block.functions {
            if binding.function == *old_id {
                binding.function = new_id.to_owned();
            }
        }
    }
    for edge in &mut ir.call_edges {
        if edge.caller == *old_id {
            edge.caller = new_id.to_owned();
        }
        if edge.callee == *old_id {
            edge.callee = new_id.to_owned();
        }
    }
    Ok(())
}
