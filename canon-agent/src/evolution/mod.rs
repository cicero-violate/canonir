pub mod lyapunov;
mod structural;
pub use lyapunov::{
    enforce_lyapunov_bound, StructureDriftError, StructureMetrics, DEFAULT_TOPOLOGY_THETA,
};
use crate::ir::{ChangePayload, CodeDelta, DeltaId, SystemState, Visibility};
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

// π: module_id → src/<module_id>.rs
fn module_path(module_id: &str) -> String {
    format!("src/{}.rs", module_id)
}

fn vis_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public   => "pub ",
        Visibility::PubCrate => "pub(crate) ",
        Visibility::PubSuper => "pub(super) ",
        Visibility::Private  => "",
    }
}

// φ: ChangePayload → CodeDelta::ApplyPatch (or Bash no-op for non-file variants)
fn payload_to_code_delta(payload: &ChangePayload) -> CodeDelta {
    match payload {
        // ── AddModule: create new file with module stub ────────────────────────
        ChangePayload::AddModule { module_id, name, visibility, description } => {
            let vis = vis_str(*visibility);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Add File: {path}\n\
                 +// {desc}\n\
                 +{vis}mod {name} {{}}\n\
                 *** End Patch\n",
                path = module_path(module_id),
                desc = description,
                vis  = vis,
                name = name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddStruct: append struct definition to module file ─────────────────
        ChangePayload::AddStruct { module, struct_id: _, name } => {
            let path = module_path(module);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +\n\
                 +pub struct {name} {{\n\
                 +}}\n\
                 *** End Patch\n",
                path = path,
                name = name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddField: append field to struct block ─────────────────────────────
        ChangePayload::AddField { struct_id, field } => {
            // Target the module that owns this struct; we don't have a direct
            // module→file mapping without the IR here, so we use struct_id as
            // the file key by convention (struct_id is "<module>::<name>").
            let module_hint = struct_id.split("::").next().unwrap_or(struct_id.as_str());
            let path = module_path(module_hint);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +    pub {field_name}: (),\n\
                 *** End Patch\n",
                path       = path,
                field_name = field.name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddTrait: append trait skeleton to module file ─────────────────────
        ChangePayload::AddTrait { module, trait_id: _, name } => {
            let path = module_path(module);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +\n\
                 +pub trait {name} {{\n\
                 +}}\n\
                 *** End Patch\n",
                path = path,
                name = name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddTraitFunction: append fn signature inside trait ─────────────────
        ChangePayload::AddTraitFunction { trait_id, function } => {
            let module_hint = trait_id.split("::").next().unwrap_or(trait_id.as_str());
            let path = module_path(module_hint);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +    fn {name}(&self);\n\
                 *** End Patch\n",
                path = path,
                name = function.name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddImpl: append impl block skeleton ───────────────────────────────
        ChangePayload::AddImpl { module, impl_id: _, struct_id, trait_id } => {
            let path = module_path(module);
            let struct_name = struct_id.split("::").last().unwrap_or(struct_id.as_str());
            let trait_name  = trait_id.split("::").last().unwrap_or(trait_id.as_str());
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +\n\
                 +impl {trait_name} for {struct_name} {{\n\
                 +}}\n\
                 *** End Patch\n",
                path        = path,
                trait_name  = trait_name,
                struct_name = struct_name,
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddFunction: append fn stub inside impl file ──────────────────────
        ChangePayload::AddFunction { function_id: _, impl_id, signature } => {
            let module_hint = impl_id.split("::").next().unwrap_or(impl_id.as_str());
            let path = module_path(module_hint);
            let vis  = vis_str(signature.visibility);
            let name = signature.name.as_str();
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +    {vis}fn {name}(&self) {{\n\
                 +        todo!()\n\
                 +    }}\n\
                 *** End Patch\n",
                path = path,
                vis  = vis,
                name = name,
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddEnum: append enum skeleton ─────────────────────────────────────
        ChangePayload::AddEnum { module, enum_id: _, name, visibility } => {
            let vis  = vis_str(*visibility);
            let path = module_path(module);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +\n\
                 +{vis}enum {name} {{\n\
                 +}}\n\
                 *** End Patch\n",
                path = path,
                vis  = vis,
                name = name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── AddEnumVariant: append variant to enum ────────────────────────────
        ChangePayload::AddEnumVariant { enum_id, variant } => {
            let module_hint = enum_id.split("::").next().unwrap_or(enum_id.as_str());
            let path = module_path(module_hint);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 +    {variant},\n\
                 *** End Patch\n",
                path    = path,
                variant = variant.name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── UpdateStructVisibility: patch pub keyword on struct line ───────────
        ChangePayload::UpdateStructVisibility { struct_id, visibility } => {
            let module_hint = struct_id.split("::").next().unwrap_or(struct_id.as_str());
            let struct_name = struct_id.split("::").last().unwrap_or(struct_id.as_str());
            let path    = module_path(module_hint);
            let new_vis = vis_str(*visibility);
            // Remove old visibility prefix lines, insert new one.
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 -struct {struct_name} {{\n\
                 +{new_vis}struct {struct_name} {{\n\
                 *** End Patch\n",
                path        = path,
                struct_name = struct_name,
                new_vis     = new_vis,
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── RemoveField: delete field line from struct ─────────────────────────
        ChangePayload::RemoveField { struct_id, field_name } => {
            let module_hint = struct_id.split("::").next().unwrap_or(struct_id.as_str());
            let path = module_path(module_hint);
            let patch = format!(
                "*** Begin Patch\n\
                 *** Update File: {path}\n\
                 @@\n\
                 -    pub {field_name}: (),\n\
                 *** End Patch\n",
                path       = path,
                field_name = field_name.as_str(),
            );
            CodeDelta::ApplyPatch { patch }
        }

        // ── RenameArtifact: sed-style rename via bash ─────────────────────────
        ChangePayload::RenameArtifact { kind, old_id, new_id } => {
            let old_name = old_id.split("::").last().unwrap_or(old_id.as_str());
            let new_name = new_id.split("::").last().unwrap_or(new_id.as_str());
            let module_hint = old_id.split("::").next().unwrap_or(old_id.as_str());
            let path = module_path(module_hint);
            let command = format!(
                "sed -i 's/\\b{old}\\b/{new}/g' {path}",
                old  = old_name,
                new  = new_name,
                path = path,
            );
            let _ = kind; // kind already drives rename_* in structural apply
            CodeDelta::Bash { command }
        }

        // ── UpdateFunctionAst / Inputs / Outputs: IR-only, no file delta ───────
        ChangePayload::UpdateFunctionAst { .. }
        | ChangePayload::UpdateFunctionInputs { .. }
        | ChangePayload::UpdateFunctionOutputs { .. }
        // ── Graph edges / events / rewards: no file mutation ──────────────────
        | ChangePayload::AddModuleEdge { .. }
        | ChangePayload::AddCallEdge { .. }
        | ChangePayload::AttachExecutionEvent { .. }
        | ChangePayload::RecordReward { .. } => {
            // No file mutation required; emit a no-op bash command so the
            // executor pipeline still gates on cargo check.
            CodeDelta::Bash { command: "true".to_string() }
        }
    }
}

pub fn apply_admitted_deltas(
    ir: &SystemState,
    admission_ids: &[String],
) -> Result<(SystemState, Vec<CodeDelta>), EvolutionError> {
    let mut next = ir.clone();
    let mut code_deltas: Vec<CodeDelta> = Vec::new();

    for aid in admission_ids {
        // Look up the StateChange by id match.
        let delta = ir
            .deltas
            .iter()
            .find(|d| &d.id == aid)
            .ok_or_else(|| EvolutionError::UnknownDelta(aid.clone()))?;

        // Apply structural mutation to IR₁.
        structural::apply_structural_delta(&mut next, delta)?;

        // φ: emit CodeDelta from payload.
        if let Some(payload) = &delta.payload {
            code_deltas.push(payload_to_code_delta(payload));
        }
    }

    Ok((next, code_deltas))
}
