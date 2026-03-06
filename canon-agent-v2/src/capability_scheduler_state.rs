#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecStep {
    CollectReady,
    Dispatch,
    ApplyResults,
    MaintainGraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecEvent {
    Continue,
    Blocked,
    Completed,
}

pub const EXEC_TRANSITIONS: [[ExecStep; 3]; 4] = {
    use ExecEvent::*;
    use ExecStep::*;
    let mut t = [[CollectReady; 3]; 4];
    t[CollectReady as usize][Continue as usize] = Dispatch;
    t[CollectReady as usize][Blocked as usize] = MaintainGraph;
    t[CollectReady as usize][Completed as usize] = MaintainGraph;
    t[Dispatch as usize][Continue as usize] = ApplyResults;
    t[Dispatch as usize][Blocked as usize] = ApplyResults;
    t[Dispatch as usize][Completed as usize] = ApplyResults;
    t[ApplyResults as usize][Continue as usize] = MaintainGraph;
    t[ApplyResults as usize][Blocked as usize] = MaintainGraph;
    t[ApplyResults as usize][Completed as usize] = MaintainGraph;
    t[MaintainGraph as usize][Continue as usize] = CollectReady;
    t[MaintainGraph as usize][Blocked as usize] = CollectReady;
    t[MaintainGraph as usize][Completed as usize] = CollectReady;
    t
};
