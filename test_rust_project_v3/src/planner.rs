pub struct Planner;

impl Planner {
    pub fn new() -> Self {
        Planner
    }

    pub fn create_plan(&self, goal: &str) -> Vec<String> {
        let mut steps = Vec::new();
        for i in 0..50 {
            steps.push(format!("Step {} for {}", i, goal));
        }
        steps
    }
}

