use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use super::reward::RewardRecord;
use super::{
    artifacts::{EnumVariant, Field, TraitFunction},
    functions::FunctionSignature,
    ids::{
        DeltaId, EnumId, ExecutionRecordId, FunctionId, ImplId, ModuleId, StructId,
        TraitId,
    },
    timeline::ExecutionEvent, types::{ValuePort, Visibility},
    word::Word,
};
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct StateChange {
    pub id: DeltaId,
    pub kind: DeltaKind,
    pub stage: super::core::PipelineStage,
    pub append_only: bool,
    pub proof: super::ids::ProofId,
    pub description: String,
    pub related_function: Option<FunctionId>,
    pub payload: Option<ChangePayload>,
    #[serde(default)]
    pub proof_object_hash: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeltaKind {
    State,
    Io,
    Structure,
    History,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangePayload {
    AddModule {
        module_id: ModuleId,
        name: Word,
        visibility: Visibility,
        description: String,
    },
    AddStruct { module: ModuleId, struct_id: StructId, name: Word },
    AddField { struct_id: StructId, field: Field },
    AddTrait { module: ModuleId, trait_id: TraitId, name: Word },
    AddTraitFunction { trait_id: TraitId, function: TraitFunction },
    AddImpl {
        module: ModuleId,
        impl_id: ImplId,
        struct_id: StructId,
        trait_id: TraitId,
    },
    AddFunction {
        function_id: FunctionId,
        impl_id: ImplId,
        signature: FunctionSignature,
    },
    AddModuleEdge { from: ModuleId, to: ModuleId, rationale: String },
    AddCallEdge { caller: FunctionId, callee: FunctionId },
    AttachExecutionEvent { execution_id: ExecutionRecordId, event: ExecutionEvent },
    UpdateFunctionAst { function_id: FunctionId, ast: JsonValue },
    AddEnum { module: ModuleId, enum_id: EnumId, name: Word, visibility: Visibility },
    AddEnumVariant { enum_id: EnumId, variant: EnumVariant },
    UpdateFunctionInputs { function_id: FunctionId, inputs: Vec<ValuePort> },
    UpdateFunctionOutputs { function_id: FunctionId, outputs: Vec<ValuePort> },
    UpdateStructVisibility { struct_id: StructId, visibility: Visibility },
    RemoveField { struct_id: StructId, field_name: Word },
    RenameArtifact { kind: String, old_id: String, new_id: String },
    RecordReward { record: RewardRecord },
}
