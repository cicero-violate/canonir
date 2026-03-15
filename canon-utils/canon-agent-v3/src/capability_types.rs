use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapabilityMode {
    Observe = 0,
    Verify = 1,
    Mutate = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineCapability {
    #[serde(alias = "CreateNode")]
    CreateNode,
    #[serde(alias = "AddEdge")]
    AddEdge,
    #[serde(alias = "UpdateStatus")]
    UpdateStatus,
    #[serde(alias = "ReadDag")]
    ReadDag,
    #[serde(alias = "ScheduleReady")]
    ScheduleReady,
    #[serde(alias = "GoalToSubgoals")]
    GoalToSubgoals,
    #[serde(alias = "ConstraintAttach")]
    ConstraintAttach,
    #[serde(alias = "RefineNode")]
    RefineNode,
    #[serde(alias = "DependencyRewrite")]
    DependencyRewrite,
    #[serde(alias = "RadiusBudgetEval")]
    RadiusBudgetEval,
    #[serde(alias = "ApplyPatch")]
    ApplyPatch,
    #[serde(alias = "FileRead")]
    FileRead,
    #[serde(alias = "FileWrite")]
    FileWrite,
    #[serde(alias = "Bash")]
    Bash,
    #[serde(alias = "CargoBuild")]
    CargoBuild,
    #[serde(alias = "CargoCheck")]
    CargoCheck,
    #[serde(alias = "StdoutCapture")]
    StdoutCapture,
    #[serde(alias = "ParseOrchestrationReport")]
    ParseOrchestrationReport,
    #[serde(alias = "DetectFailures")]
    DetectFailures,
    #[serde(alias = "StatusUpdateOnly")]
    StatusUpdateOnly,
    #[serde(alias = "ReadStructuralSurface")]
    ReadStructuralSurface,
    #[serde(alias = "ComputeDelta")]
    ComputeDelta,
    #[serde(alias = "RewardSignalCompute")]
    RewardSignalCompute,
    #[serde(alias = "InvariantCheck")]
    InvariantCheck,
    #[serde(alias = "BoundaryGuard")]
    BoundaryGuard,
    #[serde(alias = "PromptContractEnforce")]
    PromptContractEnforce,
    #[serde(alias = "Llm")]
    Llm,
    #[serde(alias = "Analysis")]
    Analysis,
    #[serde(alias = "StatelessInvoke")]
    StatelessInvoke,
    #[serde(other)]
    Unknown,
}
impl PipelineCapability {
    pub fn class(self) -> CapabilityMode {
        match self {
            PipelineCapability::FileRead => CapabilityMode::Observe,
            PipelineCapability::ReadDag => CapabilityMode::Observe,
            PipelineCapability::ReadStructuralSurface => CapabilityMode::Observe,
            PipelineCapability::StdoutCapture => CapabilityMode::Observe,
            PipelineCapability::StatelessInvoke => CapabilityMode::Observe,
            PipelineCapability::RadiusBudgetEval => CapabilityMode::Observe,
            PipelineCapability::ComputeDelta => CapabilityMode::Observe,
            PipelineCapability::RewardSignalCompute => CapabilityMode::Observe,
            PipelineCapability::PromptContractEnforce => CapabilityMode::Observe,
            PipelineCapability::Llm => CapabilityMode::Observe,
            PipelineCapability::Analysis => CapabilityMode::Observe,
            PipelineCapability::GoalToSubgoals => CapabilityMode::Observe,
            PipelineCapability::ScheduleReady => CapabilityMode::Observe,
            PipelineCapability::ConstraintAttach => CapabilityMode::Observe,
            PipelineCapability::StatusUpdateOnly => CapabilityMode::Verify,
            PipelineCapability::UpdateStatus => CapabilityMode::Verify,
            PipelineCapability::ParseOrchestrationReport => CapabilityMode::Verify,
            PipelineCapability::DetectFailures => CapabilityMode::Verify,
            PipelineCapability::InvariantCheck => CapabilityMode::Verify,
            PipelineCapability::BoundaryGuard => CapabilityMode::Verify,
            PipelineCapability::ApplyPatch => CapabilityMode::Mutate,
            PipelineCapability::FileWrite => CapabilityMode::Mutate,
            PipelineCapability::Bash => CapabilityMode::Mutate,
            PipelineCapability::CargoBuild => CapabilityMode::Mutate,
            PipelineCapability::CargoCheck => CapabilityMode::Observe,
            PipelineCapability::CreateNode => CapabilityMode::Mutate,
            PipelineCapability::AddEdge => CapabilityMode::Mutate,
            PipelineCapability::RefineNode => CapabilityMode::Mutate,
            PipelineCapability::DependencyRewrite => CapabilityMode::Mutate,
            PipelineCapability::Unknown => CapabilityMode::Observe,
        }
    }
}
pub fn capability_model_dominant_class(caps: &[PipelineCapability]) -> CapabilityMode {
    caps.iter()
        .map(|c| c.class())
        .max_by_key(|&c| c as u8)
        .unwrap_or(CapabilityMode::Observe)
}
pub fn capability_model_assert_class_disjoint(
    caps: &HashSet<PipelineCapability>,
) -> Result<(), String> {
    let has_mutate = caps.iter().any(|c| c.class() == CapabilityMode::Mutate);
    let has_verify = caps.iter().any(|c| c.class() == CapabilityMode::Verify);
    if has_mutate && has_verify {
        return Err(
            format!(
                "capability class violation: node mixes Mutate and Verify capabilities: {:?}",
                caps.iter().filter(| c | c.class() != CapabilityMode::Observe).collect::<
                Vec < _ >> ()
            ),
        );
    }
    Ok(())
}
