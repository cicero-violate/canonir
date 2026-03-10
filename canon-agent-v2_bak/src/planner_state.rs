#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerStage {
    ReuseTemplate,
    MutateTemplate,
    GraphPatch,
    Execute,
    Evaluate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerTransition {
    ReuseDone,
    MutationDone,
    PlannerDone,
    ExecuteDone,
}
pub const PLANNER_TRANSITIONS: [[PlannerStage; 4]; 5] = {
    use PlannerStage::*;
    use PlannerTransition::*;
    let mut t = [[ReuseTemplate; 4]; 5];
    t[ReuseTemplate as usize][ReuseDone as usize] = MutateTemplate;
    t[MutateTemplate as usize][MutationDone as usize] = GraphPatch;
    t[GraphPatch as usize][PlannerDone as usize] = Execute;
    t[Execute as usize][ExecuteDone as usize] = Evaluate;
    t
};
