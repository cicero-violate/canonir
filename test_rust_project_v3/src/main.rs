mod agent;
mod planner;
mod executor;
mod tools;

use agent::Agent;

fn main() {
    let mut agent = Agent::new();
    agent.run();
}

