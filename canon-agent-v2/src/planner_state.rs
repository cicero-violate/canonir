#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlannerStagePersist {
    pub stage: PlannerStage,
    pub tick: u64,
}

impl PlannerStagePersist {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(path: &std::path::Path, stage: PlannerStage, tick: u64) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let payload = PlannerStagePersist { stage, tick };
        let tmp = path.with_extension("tmp");
        if let Ok(text) = serde_json::to_string_pretty(&payload) {
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}
