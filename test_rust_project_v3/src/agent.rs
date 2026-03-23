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

    pub fn run(&mut self) {
        let plan = self.planner.create_plan("default task");
        self.executor.execute(plan);
        // Use tools to avoid dead code warnings
        let outputs = crate::tools::generate_bulk();
        println!("Generated {} tool outputs", outputs.len());
    }
}
