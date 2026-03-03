use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

pub fn mutation_caps() -> HashSet<Capability> {
    [
        Capability::ApplyPatch,
        Capability::FileWrite,
        Capability::Bash,
        Capability::CargoBuild,
        Capability::CargoCheck,
        Capability::StdoutCapture,
    ]
    .into_iter()
    .collect()
}

pub fn verify_caps() -> HashSet<Capability> {
    [
        Capability::ParseOrchestrationReport,
        Capability::DetectFailures,
        Capability::StatusUpdateOnly,
        Capability::UpdateStatus,
        Capability::InvariantCheck,
        Capability::BoundaryGuard,
    ]
    .into_iter()
    .collect()
}

pub fn assert_mut_verify_disjoint(caps: &HashSet<Capability>) -> Result<(), String> {
    let mut_caps = mutation_caps();
    let ver_caps = verify_caps();
    let overlap: HashSet<_> = caps.intersection(&mut_caps).copied().collect::<HashSet<_>>().intersection(&ver_caps).copied().collect();
    if !overlap.is_empty() {
        return Err(format!("capability overlap violation: {:?}", overlap));
    }
    Ok(())
}
