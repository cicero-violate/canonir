//! IR slice builder — extracts a typed CanonicalIr subset per IrField declaration.
//!
//! This is the enforcement mechanism for stateless call safety.
//! A node literally cannot see fields it did not declare in CapabilityNode::reads.
use super::capability::IrField;
use crate::ir::SystemState;
use serde_json::{Map, Value};
/// Maximum number of items to include per list field in a slice.
/// Keeps prompts within LLM context limits.
const MAX_LIST_ITEMS: usize = 30;
/// Truncate a JSON array to MAX_LIST_ITEMS, appending a sentinel if clipped.
fn truncate_list(v: Value) -> Value {
    match v {
        Value::Array(mut arr) => {
            let total = arr.len();
            arr.truncate(MAX_LIST_ITEMS);
            if total > MAX_LIST_ITEMS {
                arr.push(
                    Value::String(
                        format!("... ({} more items truncated)", total - MAX_LIST_ITEMS),
                    ),
                );
            }
            Value::Array(arr)
        }
        other => other,
    }
}
/// Strip heavy fields from a Function object before sending to the LLM.
/// Removes large payloads (e.g., bytecode, AST) and keeps structural data only.
fn slim_function(f: &crate::ir::Function) -> Value {
    serde_json::json!(
        { "id" : f.id, "name" : f.name, "module" : f.module, "visibility" : f.visibility,
        "inputs" : f.inputs, "outputs" : f.outputs, "deltas" : f.deltas, }
    )
}
/// Builds a JSON object containing only the IR fields listed in `fields`.
/// The returned Value is what gets handed to AgentCallInput::ir_slice.
pub fn slice_ir_fields(ir: &SystemState, fields: &[IrField]) -> Value {
    let mut map = Map::new();
    for field in fields {
        let (key, value) = extract_field(ir, field);
        map.insert(key, value);
    }
    Value::Object(map)
}
/// Extracts a single IrField from CanonicalIr as a (key, JSON value) pair.
fn extract_field(ir: &SystemState, field: &IrField) -> (String, Value) {
    match field {
        IrField::Modules => {
            (
                "modules".into(),
                truncate_list(serde_json::to_value(&ir.modules).unwrap_or(Value::Null)),
            )
        }
        IrField::ModuleEdges => {
            (
                "module_edges".into(),
                truncate_list(
                    serde_json::to_value(&ir.module_edges).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Structs => {
            (
                "structs".into(),
                truncate_list(serde_json::to_value(&ir.structs).unwrap_or(Value::Null)),
            )
        }
        IrField::Enums => {
            (
                "enums".into(),
                truncate_list(serde_json::to_value(&ir.enums).unwrap_or(Value::Null)),
            )
        }
        IrField::Traits => {
            (
                "traits".into(),
                truncate_list(serde_json::to_value(&ir.traits).unwrap_or(Value::Null)),
            )
        }
        IrField::ImplBlocks => {
            (
                "impls".into(),
                truncate_list(serde_json::to_value(&ir.impls).unwrap_or(Value::Null)),
            )
        }
        IrField::Functions => {
            (
                "functions".into(),
                truncate_list(
                    Value::Array(ir.functions.iter().map(slim_function).collect()),
                ),
            )
        }
        IrField::CallEdges => {
            (
                "call_edges".into(),
                truncate_list(
                    serde_json::to_value(&ir.call_edges).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::TickGraphs => {
            (
                "tick_graphs".into(),
                truncate_list(
                    serde_json::to_value(&ir.tick_graphs).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::SystemGraphs => {
            (
                "system_graphs".into(),
                truncate_list(
                    serde_json::to_value(&ir.system_graphs).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::LoopPolicies => {
            (
                "loop_policies".into(),
                truncate_list(
                    serde_json::to_value(&ir.loop_policies).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Ticks => {
            (
                "ticks".into(),
                truncate_list(serde_json::to_value(&ir.ticks).unwrap_or(Value::Null)),
            )
        }
        IrField::TickEpochs => {
            (
                "tick_epochs".into(),
                truncate_list(
                    serde_json::to_value(&ir.tick_epochs).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::PolicyParameters => {
            (
                "policy_parameters".into(),
                truncate_list(
                    serde_json::to_value(&ir.policy_parameters).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Plans => {
            (
                "plans".into(),
                truncate_list(serde_json::to_value(&ir.plans).unwrap_or(Value::Null)),
            )
        }
        IrField::Executions => {
            (
                "executions".into(),
                truncate_list(
                    serde_json::to_value(&ir.executions).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Admissions => {
            (
                "admissions".into(),
                truncate_list(
                    serde_json::to_value(&ir.admissions).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::AppliedDeltas => {
            (
                "applied_deltas".into(),
                truncate_list(
                    serde_json::to_value(&ir.applied_deltas).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::GpuFunctions => {
            (
                "gpu_functions".into(),
                truncate_list(
                    serde_json::to_value(&ir.gpu_functions).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Proposals => {
            (
                "proposals".into(),
                truncate_list(serde_json::to_value(&ir.proposals).unwrap_or(Value::Null)),
            )
        }
        IrField::Judgments => {
            (
                "judgments".into(),
                truncate_list(serde_json::to_value(&ir.judgments).unwrap_or(Value::Null)),
            )
        }
        IrField::JudgmentPredicates => {
            (
                "judgment_predicates".into(),
                truncate_list(
                    serde_json::to_value(&ir.judgment_predicates).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::Deltas => {
            (
                "deltas".into(),
                truncate_list(serde_json::to_value(&ir.deltas).unwrap_or(Value::Null)),
            )
        }
        IrField::Proofs => {
            (
                "proofs".into(),
                truncate_list(serde_json::to_value(&ir.proofs).unwrap_or(Value::Null)),
            )
        }
        IrField::Learning => {
            (
                "learning".into(),
                truncate_list(serde_json::to_value(&ir.learning).unwrap_or(Value::Null)),
            )
        }
        IrField::Errors => {
            (
                "errors".into(),
                truncate_list(serde_json::to_value(&ir.errors).unwrap_or(Value::Null)),
            )
        }
        IrField::Dependencies => {
            (
                "dependencies".into(),
                truncate_list(
                    serde_json::to_value(&ir.dependencies).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::FileHashes => {
            (
                "file_hashes".into(),
                truncate_list(
                    serde_json::to_value(&ir.file_hashes).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::RewardDeltas => {
            (
                "reward_deltas".into(),
                truncate_list(
                    serde_json::to_value(&ir.reward_deltas).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::WorldModel => {
            (
                "world_model".into(),
                truncate_list(
                    serde_json::to_value(&ir.world_model).unwrap_or(Value::Null),
                ),
            )
        }
        IrField::GoalMutations => {
            (
                "goal_mutations".into(),
                truncate_list(
                    serde_json::to_value(&ir.goal_mutations).unwrap_or(Value::Null),
                ),
            )
        }
    }
}
