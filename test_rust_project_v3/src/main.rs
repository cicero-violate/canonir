mod agent;
mod generated;

fn main() {
    let steps = agent::planner::plan("goal");
    agent::executor::execute(&steps);
    generated::touch();
}
