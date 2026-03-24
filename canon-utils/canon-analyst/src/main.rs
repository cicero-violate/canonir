mod agent;
mod python;

use anyhow::Result;

const TLOG_PATH: &str =
    "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d/00000000000000000000.log";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Optional: pass a question on the command line.
    // Usage: canon-analyst "why did the planner stall?"
    let question = args.get(1).cloned().unwrap_or_else(|| {
        "Analyse the event log and provide a concise diagnosis of system health, \
         any stalls or failures, and actionable recommendations."
            .to_string()
    });

    agent::run(&question, TLOG_PATH).await
}
