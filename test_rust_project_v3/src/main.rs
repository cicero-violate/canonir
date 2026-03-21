mod agent;
mod planning;
mod execution;

fn main() {
    let a = agent::Agent::new(1);
    a.run();
    let plan = planning::create_plan("goal");
    execution::execute_plan(&plan);
}
