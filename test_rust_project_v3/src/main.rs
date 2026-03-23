mod agent;
mod planner;
mod executor;
mod tools;

use agent::Agent;

fn main() {
    let agent = Agent::new();
    agent.run("test input");

    // Ensure tools are used directly to satisfy -F dead-code
    let _ = crate::tools::echo_tool("test");
    let _ = crate::tools::uppercase_tool("test");
    let _ = crate::tools::reverse_tool("test");
}
