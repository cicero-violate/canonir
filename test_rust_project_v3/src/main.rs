mod agent;
mod planner;
mod executor;
mod tools;

use agent::Agent;

fn main() {
    let agent = Agent::new();
    agent.run("test input");
}
