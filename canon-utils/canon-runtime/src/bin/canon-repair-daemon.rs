/// canon-repair-daemon — persistent repair job server.
///
/// Binds a TCP server on 127.0.0.1:9102 (or $CANON_REPAIR_SERVER_ADDR) and
/// processes incoming RepairJobRequests sequentially, one at a time.
///
/// For each job it rebuilds and spawns `canon-harness-repair`, which routes
/// LLM calls through the relay server already running in the supervisor
/// (127.0.0.1:9101 / $CANON_LLM_RELAY_ADDR).
///
/// Usage:
///   canon-repair-daemon [--addr 127.0.0.1:9102] [--workspace /path/to/workspace]
///
/// The daemon runs until killed.  harness_suite (or any other caller) submits
/// jobs via repair_client_submit().

use anyhow::{anyhow, bail, Context, Result};
use canon_llm::repair_server::{
    repair_server_start, RepairJobResult, REPAIR_SERVER_ADDR,
};
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const WORKSPACE_REPAIR_SENTINEL: &str = "__workspace__";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut addr = std::env::var("CANON_REPAIR_SERVER_ADDR")
        .unwrap_or_else(|_| REPAIR_SERVER_ADDR.to_string());
    let mut workspace = PathBuf::from(
        std::env::var("CANON_WORKSPACE").unwrap_or_else(|_| DEFAULT_WORKSPACE.to_string()),
    );

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => {
                addr = args.next().ok_or_else(|| anyhow!("missing value for --addr"))?;
            }
            "--workspace" => {
                workspace = PathBuf::from(
                    args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?,
                );
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    // Ensure canon-harness-repair is up to date before accepting any jobs.
    rebuild_harness_repair(&workspace)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    runtime.block_on(async move {
        let workspace_for_fn = workspace.clone();

        let handle = repair_server_start(&addr, move |req| {
            let workspace = workspace_for_fn.clone();
            Box::pin(async move {
                if req.crate_name == WORKSPACE_REPAIR_SENTINEL
                    && req.test_name == WORKSPACE_REPAIR_SENTINEL
                {
                    eprintln!(
                        "[canon-repair-daemon] running workspace incident repair (max_steps={})",
                        req.max_steps
                    );
                    let workspace_clone = workspace.clone();
                    let max_steps = req.max_steps;
                    let failure_output = req.failure_output.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        run_workspace_incident_repair(&workspace_clone, max_steps, &failure_output)
                    }).await;

                    return match result {
                        Ok(Ok(())) => RepairJobResult {
                            success: true,
                            steps_taken: req.max_steps,
                            error: None,
                        },
                        Ok(Err(e)) => RepairJobResult {
                            success: false,
                            steps_taken: req.max_steps,
                            error: Some(e.to_string()),
                        },
                        Err(e) => RepairJobResult {
                            success: false,
                            steps_taken: 0,
                            error: Some(format!("spawn_blocking panicked: {e}")),
                        },
                    };
                }

                let bin = workspace.join("target/debug/canon-harness-repair");

                // Write the failure output to the standard state path so the
                // repair binary can read it via --stderr-file.
                let state_dir = workspace.join("state");
                let _ = std::fs::create_dir_all(&state_dir);
                let stderr_path = state_dir.join("harness_suite_failure.txt");
                if let Err(e) = std::fs::write(&stderr_path, &req.failure_output) {
                    return RepairJobResult {
                        success: false,
                        steps_taken: 0,
                        error: Some(format!("failed to write failure output: {e}")),
                    };
                }

                eprintln!(
                    "[canon-repair-daemon] running repair for {}::{} (max_steps={})",
                    req.crate_name, req.test_name, req.max_steps
                );

                let mut cmd = Command::new(&bin);
                cmd.arg(&req.crate_name)
                    .arg(&req.test_name)
                    .arg("--workspace")
                    .arg(&workspace)
                    .arg("--stderr-file")
                    .arg(&stderr_path)
                    .arg("--max-steps")
                    .arg(req.max_steps.to_string());

                if let Some(ctx) = &req.incident_context {
                    let incident_path = state_dir.join("repair_incident_context.txt");
                    if let Err(e) = std::fs::write(&incident_path, ctx) {
                        return RepairJobResult {
                            success: false,
                            steps_taken: 0,
                            error: Some(format!("failed to write incident context: {e}")),
                        };
                    }
                    cmd.arg("--incident-file").arg(&incident_path);
                }

                // Run synchronously — the repair binary drives the LLM loop.
                let result = tokio::task::spawn_blocking(move || cmd.status()).await;

                match result {
                    Ok(Ok(status)) if status.success() => {
                        eprintln!(
                            "[canon-repair-daemon] repair succeeded for {}::{}",
                            req.crate_name, req.test_name
                        );
                        RepairJobResult { success: true, steps_taken: req.max_steps, error: None }
                    }
                    Ok(Ok(status)) => {
                        let msg = format!(
                            "canon-harness-repair exited with status {} for {}::{}",
                            status, req.crate_name, req.test_name
                        );
                        eprintln!("[canon-repair-daemon] {msg}");
                        RepairJobResult { success: false, steps_taken: req.max_steps, error: Some(msg) }
                    }
                    Ok(Err(e)) => {
                        let msg = format!("failed to spawn canon-harness-repair: {e}");
                        eprintln!("[canon-repair-daemon] {msg}");
                        RepairJobResult { success: false, steps_taken: 0, error: Some(msg) }
                    }
                    Err(e) => {
                        let msg = format!("spawn_blocking panicked: {e}");
                        eprintln!("[canon-repair-daemon] {msg}");
                        RepairJobResult { success: false, steps_taken: 0, error: Some(msg) }
                    }
                }
            })
        })
        .await
        .context("failed to start repair job server")?;

        eprintln!("[canon-repair-daemon] listening on {}", handle.local_addr());
        eprintln!("[canon-repair-daemon] ready — waiting for repair jobs (Ctrl-C to stop)");

        // Block forever; drop(handle) on Ctrl-C shuts down the server.
        tokio::signal::ctrl_c().await.ok();
        eprintln!("[canon-repair-daemon] shutting down");
        drop(handle);
        Ok::<(), anyhow::Error>(())
    })
}

fn run_workspace_incident_repair(
    workspace: &PathBuf,
    max_steps: usize,
    failure_output: &str,
) -> Result<()> {
    let state_dir = workspace.join("state");
    std::fs::create_dir_all(&state_dir)?;
    let eventlog_path = state_dir.join("event_trigger_failure.txt");
    std::fs::write(&eventlog_path, failure_output)
        .context("failed to write workspace incident failure output")?;
    let mut cmd = Command::new(workspace.join("target/debug/canon-eventlog-repair"));
    cmd.arg("--workspace")
        .arg(workspace)
        .arg("--crate")
        .arg("canon-runtime")
        .arg("--test")
        .arg("synthetic_event_trigger_incident")
        .arg("--event-jsonl")
        .arg(&eventlog_path)
        .arg("--max-steps")
        .arg(max_steps.to_string())
        .arg("--event-tlog")
        .arg(workspace.join("state/event_log/event.tlog.d"));

    let status = cmd
        .status()
        .context("failed to run canon-eventlog-repair for workspace incident")?;

    if status.success() {
        Ok(())
    } else {
        bail!("canon-eventlog-repair exited with status {}", status);
    }
}

fn rebuild_harness_repair(workspace: &PathBuf) -> Result<()> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("canon-runtime")
        .arg("--bin")
        .arg("canon-harness-repair")
        .arg("--bin")
        .arg("canon-eventlog-repair")
        .current_dir(workspace)
        .status()
        .context("failed to run cargo build for repair binaries")?;

    if !status.success() {
        bail!("rebuilding canon-harness-repair failed with status {}", status);
    }
    Ok(())
}
