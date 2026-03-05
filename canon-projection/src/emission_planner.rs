use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct EmissionTask {
    pub module: String,
    pub path: PathBuf,
}

#[derive(Default, Debug)]
pub struct EmissionPlanner {
    tasks: Vec<EmissionTask>,
}

impl EmissionPlanner {
    pub fn add(&mut self, module: impl Into<String>, path: PathBuf) {
        self.tasks.push(EmissionTask { module: module.into(), path });
    }

    pub fn tasks(&self) -> &[EmissionTask] {
        &self.tasks
    }
}
