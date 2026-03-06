#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerPhase {
    ReuseTemplate,
    MutateTemplate,
    PlannerUpdate,
    Execute,
    Evaluate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerEvent {
    ReuseDone,
    MutationDone,
    PlannerDone,
    ExecuteDone,
}

pub const PLANNER_TRANSITIONS: [[PlannerPhase; 4]; 5] = {
    use PlannerEvent::*;
    use PlannerPhase::*;
    let mut t = [[ReuseTemplate; 4]; 5];
    t[ReuseTemplate as usize][ReuseDone as usize] = MutateTemplate;
    t[MutateTemplate as usize][MutationDone as usize] = PlannerUpdate;
    t[PlannerUpdate as usize][PlannerDone as usize] = Execute;
    t[Execute as usize][ExecuteDone as usize] = Evaluate;
    t
};
