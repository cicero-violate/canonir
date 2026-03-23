pub struct Executor;

impl Executor {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, steps: Vec<String>) -> String {
        steps.join(" -> ")
    }
}

