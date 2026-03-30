use anyhow::{anyhow, bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_ROUNDS: usize = 3;
const DEFAULT_MAX_STEPS: usize = 30;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut crate_name: Option<String> = None;
    let mut test_name: Option<String> = None;
    let mut event_jsonl: Option<PathBuf> = None;
    let mut max_rounds = DEFAULT_MAX_ROUNDS;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                workspace = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?);
            }
            "--crate" => {
                crate_name = Some(args.next().ok_or_else(|| anyhow!("missing value for --crate"))?);
            }
            "--test" => {
                test_name = Some(args.next().ok_or_else(|| anyhow!("missing value for --test"))?);
            }
            "--event-jsonl" => {
                event_jsonl = Some(PathBuf::from(args.next().ok_or_else(|| anyhow!("missing value for --event-jsonl"))?));
            }
            "--max-rounds" => {
                max_rounds = args.next().ok_or_else(|| anyhow!("missing value for --max-rounds"))?.parse().context("--max-rounds must be an integer")?;
            }
            "--max-steps" => {
                max_steps = args.next().ok_or_else(|| anyhow!("missing value for --max-steps"))?.parse().context("--max-steps must be an integer")?;
            }
            "--dry-run" => dry_run = true,
            other => bail!("unknown argument: {other}"),
        }
    }

    let crate_name = crate_name.ok_or_else(|| anyhow!("missing --crate"))?;
    let test_name = test_name.ok_or_else(|| anyhow!("missing --test"))?;
    let event_jsonl = event_jsonl.ok_or_else(|| anyhow!("missing --event-jsonl"))?;

    build_eventlog_repair(&workspace)?;

    for round in 1..=max_rounds {
        eprintln!("[canon-eventlog-harness-suite] round {} repairing {}::{} from {}", round, crate_name, test_name, event_jsonl.display());
        let mut cmd = Command::new(workspace.join("target/debug/canon-eventlog-repair"));
        cmd.arg("--workspace").arg(&workspace).arg("--crate").arg(&crate_name).arg("--test").arg(&test_name).arg("--event-jsonl").arg(&event_jsonl).arg("--max-steps").arg(max_steps.to_string());
        if dry_run {
            cmd.arg("--dry-run");
        }

        let status = cmd.status().with_context(|| "failed to run canon-eventlog-repair")?;

        if !status.success() {
            bail!("canon-eventlog-repair failed in round {} with status {}", round, status);
        }

        if dry_run {
            return Ok(());
        }

        let verify = Command::new("cargo")
            .arg("test")
            .arg("-p")
            .arg(&crate_name)
            .arg(&test_name)
            .arg("--")
            .arg("--nocapture")
            .current_dir(&workspace)
            .status()
            .with_context(|| format!("failed to verify {}::{}", crate_name, test_name))?;

        if verify.success() {
            println!("eventlog harness suite complete: {}::{} passed after {} round(s)", crate_name, test_name, round);
            return Ok(());
        }
    }

    bail!("eventlog harness suite stopped after {} round(s) without passing {}::{}", max_rounds, crate_name, test_name)
}

fn build_eventlog_repair(workspace: &PathBuf) -> Result<()> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("canon-runtime")
        .arg("--bin")
        .arg("canon-eventlog-repair")
        .current_dir(workspace)
        .status()
        .context("failed to rebuild canon-eventlog-repair")?;
    if status.success() {
        Ok(())
    } else {
        bail!("rebuilding canon-eventlog-repair failed with status {}", status);
    }
}
