pub struct Planner;

impl Planner {
    pub fn new() -> Self {
        Self
    }

    pub fn plan(&self, input: &str) -> Vec<String> {
        vec![format!("Process input: {}", input)]
    }
}

