use anyhow::{anyhow, bail, Context, Result};
use canon_llm::repair_server::{repair_client_submit, RepairJobRequest, REPAIR_SERVER_ADDR};
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_ROUNDS: usize = 0;
const DEFAULT_MAX_STEPS_PER_TEST: usize = 1000;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut crate_name: Option<String> = None;

    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut max_rounds = DEFAULT_MAX_ROUNDS;
    let mut max_steps_per_test = DEFAULT_MAX_STEPS_PER_TEST;
    let mut forever = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--crate" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --crate"))?;
                crate_name = Some(value);
            }
            "--workspace" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?;
                workspace = PathBuf::from(value);
            }
            "--max-rounds" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --max-rounds"))?;
                max_rounds = value.parse().context("--max-rounds must be an integer")?;
            }
            "--max-steps-per-test" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --max-steps-per-test"))?;
                max_steps_per_test = value.parse().context("--max-steps-per-test must be an integer")?;
            }
            "--forever" => {
                forever = true;
            }
            other if other.starts_with("--") => bail!("unknown argument: {other}"),
            other => {
                if crate_name.is_none() {
                    crate_name = Some(other.to_string());
                } else {
                    bail!("unexpected positional argument: {other}");
                }
            }
        }
    }

    let mut round = 1usize;
    loop {
        let suite = run_suite_tests(&workspace, crate_name.as_deref())?;
        if suite.success {
            match crate_name.as_deref() {
                Some(name) => println!("harness suite complete: cargo test -p {} passed after {} round(s)", name, round - 1),
                None => println!("harness suite complete: cargo test --workspace passed after {} round(s)", round - 1),
            }
            return Ok(());
        }

        match crate_name.as_deref() {
            Some(name) => eprintln!("[canon-harness-suite] cargo test -p {} failed in round {}", name, round),
            None => eprintln!("[canon-harness-suite] cargo test --workspace failed in round {}", round),
        }

        let Some((repair_crate, failing_test)) = first_failing_case(&suite, crate_name.as_deref()) else {
            match crate_name.as_deref() {
                Some(name) => bail!("cargo test -p {} failed, but no failing test name could be parsed\n{}", name, truncate(&suite.output, 4000)),
                None => bail!("cargo test --workspace failed, but no failing crate/test could be parsed\n{}", truncate(&suite.output, 4000)),
            };
        };

        eprintln!("[canon-harness-suite] round {} repairing {}::{}", round, repair_crate, failing_test);
        if let Err(err) = run_harness_repair(&workspace, &repair_crate, &failing_test, &suite.output, max_steps_per_test) {
            eprintln!("[canon-harness-suite] harness repair failed for {}::{}: {}", repair_crate, failing_test, err);
        }

        round += 1;
        if !forever && max_rounds != 0 && round > max_rounds {
            break;
        }
    }

    match crate_name.as_deref() {
        Some(name) => {
            eprintln!("[canon-harness-suite] failed after {} round(s) for crate {}", max_rounds, name);
            bail!("harness suite stopped after {} round(s) without passing cargo test -p {}", max_rounds, name)
        }
        None => {
            eprintln!("[canon-harness-suite] failed after {} round(s) for workspace", max_rounds);
            bail!("harness suite stopped after {} round(s) without passing cargo test --workspace", max_rounds)
        }
    }
}

/// Parses the crate name from a cargo compile-error line such as:
///   error: could not compile `canon-llm-runtime` (lib test) due to …
///   error: could not compile `canon-llm-runtime` due to …
fn crate_from_compile_error(text: &str) -> Option<String> {
    let marker = "could not compile `";
    for line in text.lines() {
        let Some(idx) = line.find(marker) else { continue };
        let rest = &line[idx + marker.len()..];
        let end = rest.find('`')?;
        let name = rest[..end].trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Returns the first `error[…]` or `error:` line from cargo output
fn compile_error_first_line(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("error[") || t.starts_with("error: ") {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_from_compile_error_standard() {
        let output = concat!("error[E0599]: no function found\n", "   --> src/relay.rs:10:5\n", "error: could not compile `canon-llm-runtime` (lib test) ", "due to 1 previous error\n",);
        assert_eq!(crate_from_compile_error(output), Some("canon-llm-runtime".to_string()));
    }

    #[test]
    fn test_crate_from_compile_error_short_form() {
        let output = "error: could not compile `my-crate` due to 3 previous errors\n";
        assert_eq!(crate_from_compile_error(output), Some("my-crate".to_string()));
    }

    #[test]
    fn test_crate_from_compile_error_none_when_no_compile_error() {
        let output = "test foo ... FAILED\nfailures:\nfoo\n";
        assert_eq!(crate_from_compile_error(output), None);
    }

    #[test]
    fn test_compile_error_first_line_bracket_form() {
        let output = "warning: unused\nerror[E0599]: foo not found\n";
        let result = compile_error_first_line(output);
        assert!(result.as_deref().unwrap_or("").starts_with("error[E0599]"));
    }

    #[test]
    fn test_compile_error_first_line_plain_form() {
        let output = "warning: x\nerror: could not compile `foo`\n";
        let result = compile_error_first_line(output);
        assert!(result.as_deref().unwrap_or("").starts_with("error:"));
    }
}

struct CommandResult {
    success: bool,
    stdout: String,
    stderr: String,
    output: String,
}

fn run_suite_tests(workspace: &PathBuf, crate_name: Option<&str>) -> Result<CommandResult> {
    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if let Some(crate_name) = crate_name {
        cmd.arg("-p").arg(crate_name);
    } else {
        cmd.arg("--workspace");
    }
    let output = cmd.arg("--").arg("--nocapture").current_dir(workspace).output().with_context(|| match crate_name {
        Some(name) => format!("failed to run cargo test -p {name}"),
        None => "failed to run cargo test --workspace".to_string(),
    })?;
    Ok(CommandResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        output: combine_output(&output.stdout, &output.stderr),
    })
}

fn run_harness_repair(workspace: &PathBuf, crate_name: &str, test_name: &str, suite_output: &str, max_steps_per_test: usize) -> Result<()> {
    let server_addr = std::env::var("CANON_REPAIR_SERVER_ADDR").unwrap_or_else(|_| REPAIR_SERVER_ADDR.to_string());
    let req = RepairJobRequest {
        crate_name: crate_name.to_string(),
        test_name: test_name.to_string(),
        failure_output: suite_output.to_string(),
        incident_context: None,
        max_steps: max_steps_per_test,
        workspace: workspace.display().to_string(),
    };

    let result = match repair_client_submit(&server_addr, &req) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("[canon-harness-suite] daemon submit failed at {}: {}; falling back to local canon-harness-repair", server_addr, err);
            return run_harness_repair_local(workspace, crate_name, test_name, suite_output, max_steps_per_test);
        }
    };

    if result.success {
        Ok(())
    } else {
        bail!("repair daemon reported failure for {}::{} after {} step(s): {}", crate_name, test_name, result.steps_taken, result.error.unwrap_or_else(|| "unknown repair failure".to_string()))
    }
}

fn run_harness_repair_local(workspace: &PathBuf, crate_name: &str, test_name: &str, suite_output: &str, max_steps_per_test: usize) -> Result<()> {
    let state_dir = workspace.join("state");
    std::fs::create_dir_all(&state_dir)?;
    let stderr_path = state_dir.join("harness_suite_failure.txt");
    std::fs::write(&stderr_path, suite_output)?;

    let status = Command::new(workspace.join("target/debug/canon-harness-repair"))
        .arg(crate_name)
        .arg(test_name)
        .arg("--workspace")
        .arg(workspace)
        .arg("--stderr-file")
        .arg(&stderr_path)
        .arg("--max-steps")
        .arg(max_steps_per_test.to_string())
        .status()
        .with_context(|| "failed to run local canon-harness-repair fallback")?;

    if status.success() {
        Ok(())
    } else {
        bail!("local canon-harness-repair failed for {}::{} with status {}", crate_name, test_name, status)
    }
}

fn first_failing_case(result: &CommandResult, default_crate: Option<&str>) -> Option<(String, String)> {
    if let Some(found) = parse_failing_case_from_text(&result.output, default_crate) {
        return Some(found);
    }
    if let Some(test_name) = first_failed_test_name(&result.stdout) {
        if let Some(crate_name) = infer_crate_from_failure_output(&result.stderr, &result.stdout, default_crate) {
            return Some((crate_name, test_name));
        }
    }
    if let Some(test_name) = first_failed_test_name(&result.stderr) {
        if let Some(crate_name) = infer_crate_from_failure_output(&result.stderr, &result.stdout, default_crate) {
            return Some((crate_name, test_name));
        }
    }
    // Fall back to compile-error detection: if cargo reported a build failure,
    // extract the crate name and use the first error line as the synthetic test name.
    if let Some(crate_name) = crate_from_compile_error(&result.output).or_else(|| default_crate.map(str::to_string)) {
        if let Some(error_summary) = compile_error_first_line(&result.output) {
            return Some((crate_name, error_summary));
        }
    }
    None
}

fn parse_failing_case_from_text(output: &str, default_crate: Option<&str>) -> Option<(String, String)> {
    let mut current_crate = default_crate.map(str::to_string);
    let mut in_failures_section = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(crate_name) = parse_running_crate(trimmed) {
            current_crate = Some(crate_name);
            in_failures_section = false;
            continue;
        }
        if trimmed == "failures:" {
            in_failures_section = true;
            continue;
        }
        if trimmed.starts_with("test result: ") {
            in_failures_section = false;
        }
        if let Some(test_name) = parse_failed_test_name(trimmed, in_failures_section) {
            if let Some(crate_name) = current_crate.clone() {
                return Some((crate_name, test_name));
            }
        }
    }
    None
}

fn first_failed_test_name(text: &str) -> Option<String> {
    let mut in_failures_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures_section = true;
            continue;
        }
        if trimmed.starts_with("test result: ") {
            in_failures_section = false;
        }
        if let Some(name) = parse_failed_test_name(trimmed, in_failures_section) {
            return Some(name);
        }
    }
    None
}

fn infer_crate_from_failure_output(stderr: &str, stdout: &str, default_crate: Option<&str>) -> Option<String> {
    if let Some(crate_name) = crate_from_rerun_hint(stderr) {
        return Some(crate_name);
    }
    if let Some(crate_name) = crate_from_workspace_source_path(stderr) {
        return Some(crate_name);
    }
    if let Some(crate_name) = crate_from_workspace_source_path(stdout) {
        return Some(crate_name);
    }
    if let Some(crate_name) = last_running_crate(stdout) {
        return Some(crate_name);
    }
    default_crate.map(str::to_string)
}

fn crate_from_workspace_source_path(text: &str) -> Option<String> {
    for line in text.lines() {
        let marker = "/workspace/ai_sandbox/canon/";
        let Some(idx) = line.find(marker) else {
            continue;
        };
        let suffix = &line[idx + marker.len()..];
        let mut parts = suffix.split('/');
        let first = parts.next()?.trim();
        if first.is_empty() {
            continue;
        }
        if first == "canon-utils" {
            let nested = parts.next()?.trim();
            if !nested.is_empty() {
                return Some(nested.to_string());
            }
            continue;
        }
        if first == "target" {
            continue;
        }
        return Some(first.to_string());
    }
    None
}

fn crate_from_rerun_hint(text: &str) -> Option<String> {
    let marker = "to rerun pass `-p ";
    for line in text.lines() {
        let Some(idx) = line.find(marker) else {
            continue;
        };
        let suffix = &line[idx + marker.len()..];
        let crate_name = suffix.split([' ', '`']).next()?.trim();
        if !crate_name.is_empty() {
            return Some(crate_name.to_string());
        }
    }
    None
}

fn last_running_crate(stdout: &str) -> Option<String> {
    stdout.lines().filter_map(|line| parse_running_crate(line.trim())).last()
}

fn parse_running_crate(line: &str) -> Option<String> {
    let prefix = "Running unittests ";
    let idx = line.find(prefix)?;
    let deps_marker = "(target/debug/deps/";
    let deps_idx = line[idx..].find(deps_marker)? + idx + deps_marker.len();
    let suffix = &line[deps_idx..];
    let crate_token = suffix.split(['-', ')']).next()?.trim();
    if crate_token.is_empty() {
        None
    } else {
        Some(crate_token.replace('_', "-"))
    }
}

fn parse_failed_test_name(line: &str, in_failures_section: bool) -> Option<String> {
    if line.starts_with("test ") && line.ends_with("FAILED") {
        let test_name = line.trim_start_matches("test ").trim_end_matches("FAILED").trim().trim_end_matches("...").trim_end_matches('.').trim();
        if !test_name.is_empty() {
            return Some(test_name.to_string());
        }
    }

    if line.starts_with("---- ") && line.ends_with(" stdout ----") {
        let test_name = line.trim_start_matches("---- ").trim_end_matches(" stdout ----").trim();
        if !test_name.is_empty() {
            return Some(test_name.to_string());
        }
    }

    if in_failures_section && !line.is_empty() && !line.starts_with("failures:") && !line.starts_with("---- ") && !line.starts_with("test result:") {
        return Some(line.to_string());
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
