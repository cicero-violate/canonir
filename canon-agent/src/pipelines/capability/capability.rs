use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapabilityClass {
    Observe = 0,
    Verify = 1,
    Mutate = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    CreateNode,
    AddEdge,
    UpdateStatus,
    ReadDag,
    ScheduleReady,
    GoalToSubgoals,
    ConstraintAttach,
    RefineNode,
    DependencyRewrite,
    RadiusBudgetEval,
    ApplyPatch,
    FileRead,
    FileWrite,
    Bash,
    CargoBuild,
    CargoCheck,
    StdoutCapture,
    ParseOrchestrationReport,
    DetectFailures,
    StatusUpdateOnly,
    ReadStructuralSurface,
    ComputeDelta,
    RewardSignalCompute,
    InvariantCheck,
    BoundaryGuard,
    PromptContractEnforce,
    StatelessInvoke,
    #[serde(other)]
    Unknown,
}

impl Capability {
    pub fn class(self) -> CapabilityClass {
        match self {
            Capability::FileRead => CapabilityClass::Observe,
            Capability::ReadDag => CapabilityClass::Observe,
            Capability::ReadStructuralSurface => CapabilityClass::Observe,
            Capability::StdoutCapture => CapabilityClass::Observe,
            Capability::StatelessInvoke => CapabilityClass::Observe,
            Capability::RadiusBudgetEval => CapabilityClass::Observe,
            Capability::ComputeDelta => CapabilityClass::Observe,
            Capability::RewardSignalCompute => CapabilityClass::Observe,
            Capability::PromptContractEnforce => CapabilityClass::Observe,
            Capability::GoalToSubgoals => CapabilityClass::Observe,
            Capability::ScheduleReady => CapabilityClass::Observe,

            Capability::StatusUpdateOnly => CapabilityClass::Verify,
            Capability::UpdateStatus => CapabilityClass::Verify,
            Capability::ParseOrchestrationReport => CapabilityClass::Verify,
            Capability::DetectFailures => CapabilityClass::Verify,
            Capability::InvariantCheck => CapabilityClass::Verify,
            Capability::BoundaryGuard => CapabilityClass::Verify,

            Capability::ApplyPatch => CapabilityClass::Mutate,
            Capability::FileWrite => CapabilityClass::Mutate,
            Capability::Bash => CapabilityClass::Mutate,
            Capability::CargoBuild => CapabilityClass::Mutate,
            Capability::CargoCheck => CapabilityClass::Mutate,
            Capability::CreateNode => CapabilityClass::Mutate,
            Capability::AddEdge => CapabilityClass::Mutate,
            Capability::RefineNode => CapabilityClass::Mutate,
            Capability::DependencyRewrite => CapabilityClass::Mutate,
            Capability::ConstraintAttach => CapabilityClass::Mutate,

            Capability::Unknown => CapabilityClass::Observe,
        }
    }
}

pub fn dominant_class(caps: &[Capability]) -> CapabilityClass {
    caps.iter()
        .map(|c| c.class())
        .max_by_key(|&c| c as u8)
        .unwrap_or(CapabilityClass::Observe)
}

pub fn assert_class_disjoint(caps: &HashSet<Capability>) -> Result<(), String> {
    let has_mutate = caps.iter().any(|c| c.class() == CapabilityClass::Mutate);
    let has_verify = caps.iter().any(|c| c.class() == CapabilityClass::Verify);
    if has_mutate && has_verify {
        return Err(format!(
            "capability class violation: node mixes Mutate and Verify capabilities: {:?}",
            caps.iter().filter(|c| c.class() != CapabilityClass::Observe).collect::<Vec<_>>()
        ));
    }
    Ok(())
}
