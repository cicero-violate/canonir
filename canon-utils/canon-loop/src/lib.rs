pub mod compiler_hints;
pub mod context;
pub mod env_model;
pub mod exec_constraints;
pub mod executor;
pub mod harness_repair;
pub mod merge;
pub mod planning_preconditions;
pub mod policy;
pub mod result;
pub mod scheduler;
pub mod stage;

#[cfg(test)]
mod tests_env_model;

pub use context::LoopContext;
pub use executor::LoopStageExecutor;
pub use harness_repair::{
    build_harness_repair_directive,
    evaluate_harness_repair_loop,
    HarnessRepairAction,
    HarnessRepairDecision,
    HarnessRepairDirective,
    HarnessRepairPhase,
    HarnessRepairState,
    HarnessRepairTarget,

    // ensure full state space visibility for exhaustive mapping tests
    *,
};
pub use result::LoopStageResult;
pub use stage::LoopStageEvent;
