use anyhow::{anyhow, bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_ROUNDS: usize = 10;
const DEFAULT_MAX_STEPS_PER_TEST: usize = 8;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let crate_name = args.next().ok_or_else(|| anyhow!("missing <crate>"))?;

    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut max_rounds = DEFAULT_MAX_ROUNDS;
    let mut max_steps_per_test = DEFAULT_MAX_STEPS_PER_TEST;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?;
                workspace = PathBuf::from(value);
            }
            "--max-rounds" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --max-rounds"))?;
                max_rounds = value.parse().context("--max-rounds must be an integer")?;
            }
            "--max-steps-per-test" => {
                let value =
                    args.next().ok_or_else(|| anyhow!("missing value for --max-steps-per-test"))?;
                max_steps_per_test =
                    value.parse().context("--max-steps-per-test must be an integer")?;
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    for round in 1..=max_rounds {
        let suite = run_crate_tests(&workspace, &crate_name)?;
        if suite.success {
            println!(
                "harness suite complete: cargo test -p {} passed after {} round(s)",
                crate_name,
                round - 1
            );
            return Ok(());
        }

        eprintln!(
            "[canon-harness-suite] cargo test -p {} failed in round {}",
            crate_name, round
        );

        let Some(failing_test) = first_failing_test(&suite.output) else {
            bail!(
                "cargo test -p {} failed, but no failing test name could be parsed\n{}",
                crate_name,
                truncate(&suite.output, 4000)
            );
        };

        eprintln!(
            "[canon-harness-suite] round {} repairing {}::{}",
            round, crate_name, failing_test
        );
        run_harness_repair(
            &workspace,
            &crate_name,
            &failing_test,
            &suite.output,
            max_steps_per_test,
        )?;
    }

    eprintln!(
        "[canon-harness-suite] failed after {} round(s) for crate {}",
        max_rounds, crate_name
    );
    bail!(
        "harness suite stopped after {} round(s) without passing cargo test -p {}",
        max_rounds,
        crate_name
    )
}

struct CommandResult {
    success: bool,
    output: String,
}

fn run_crate_tests(workspace: &PathBuf, crate_name: &str) -> Result<CommandResult> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("-p")
        .arg(crate_name)
        .arg("--")
        .arg("--nocapture")
        .current_dir(workspace)
        .output()
        .with_context(|| format!("failed to run cargo test -p {crate_name}"))?;
    Ok(CommandResult {
        success: output.status.success(),
        output: combine_output(&output.stdout, &output.stderr),
    })
}

fn run_harness_repair(
    workspace: &PathBuf,
    crate_name: &str,
    test_name: &str,
    suite_output: &str,
    max_steps_per_test: usize,
) -> Result<()> {
    let build_status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("canon-runtime")
        .arg("--bin")
        .arg("canon-harness-repair")
        .current_dir(workspace)
        .status()
        .context("failed to rebuild canon-harness-repair")?;
    if !build_status.success() {
        bail!("rebuilding canon-harness-repair failed with status {}", build_status);
    }

    let stderr_path = workspace.join("state/harness_suite_failure.txt");
    std::fs::create_dir_all(
        stderr_path
            .parent()
            .ok_or_else(|| anyhow!("invalid stderr cache path"))?,
    )?;
    std::fs::write(&stderr_path, suite_output)?;

    let bin = workspace.join("target/debug/canon-harness-repair");
    let status = Command::new(&bin)
        .arg(crate_name)
        .arg(test_name)
        .arg("--workspace")
        .arg(workspace)
        .arg("--stderr-file")
        .arg(&stderr_path)
        .arg("--max-steps")
        .arg(max_steps_per_test.to_string())
        .status()
        .with_context(|| format!("failed to run {}", bin.display()))?;

    if !status.success() {
        bail!(
            "canon-harness-repair failed for {}::{} with status {}",
            crate_name,
            test_name,
            status
        );
    }
    Ok(())
}

fn first_failing_test(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("test ") || !trimmed.ends_with(" ... FAILED") {
            continue;
        }
        let name = trimmed
            .trim_start_matches("test ")
            .trim_end_matches(" ... FAILED")
            .trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(stderr));
    }
    text
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        format!("{}...", &text[..limit])
    }
}
