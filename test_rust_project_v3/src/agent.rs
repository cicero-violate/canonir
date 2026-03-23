use crate::planner::Planner;
use crate::executor::Executor;

pub struct Agent {
    planner: Planner,
    executor: Executor,
}

impl Agent {
    pub fn new() -> Self {
        Self {
            planner: Planner::new(),
            executor: Executor::new(),
        }
    }

    pub fn run(&self, input: &str) {
        let plan = self.planner.plan(input);
        let result = self.executor.execute(plan);
        println!("Agent result: {}", result);
    }
}

