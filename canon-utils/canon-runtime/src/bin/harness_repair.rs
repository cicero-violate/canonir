use anyhow::{anyhow, Result};
use canon_event::{
    resolve_tlog_path, write_shaped_event_auto, CanonPayloadMeta, EventKind, RequestDispatch,
};
use canon_loop::{HarnessRepairTarget, LoopStageExecutor};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let crate_name = args.next().ok_or_else(|| anyhow!("missing <crate>"))?;
    let test_name = args.next().ok_or_else(|| anyhow!("missing <test-name>"))?;

    let mut always_dispatch = false;
    let mut stderr_file: Option<PathBuf> = None;
    let mut tlog_path: Option<PathBuf> = None;
    let mut workspace = PathBuf::from("/workspace/ai_sandbox/canon");

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--always-dispatch" => always_dispatch = true,
            "--stderr-file" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --stderr-file"))?;
                stderr_file = Some(PathBuf::from(value));
            }
            "--tlog" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --tlog"))?;
                tlog_path = Some(PathBuf::from(value));
            }
            "--workspace" => {
                let value = args.next().ok_or_else(|| anyhow!("missing value for --workspace"))?;
                workspace = PathBuf::from(value);
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }

    let stderr = if let Some(path) = stderr_file {
        std::fs::read_to_string(path)?
    } else {
        run_target_test(&workspace, &crate_name, &test_name, always_dispatch)?
    };

    let target = HarnessRepairTarget::new(Some(crate_name.clone()), Some(test_name.clone()));
    let mut executor = LoopStageExecutor::new(
        workspace.clone(),
        resolve_tlog_path(Some(workspace.as_path()), Some("CANON_TLOG_PATH")),
    );
    let directive = executor.evaluate_harness_repair_for_target(&target, &stderr);
    let prompt = format!(
        "Harness self-repair target:\n- crate: {crate_name}\n- failing test: {test_name}\n\nFailure output:\n{}\n\nExecute exactly one constrained repair step.\n- next phase: {:?}\n- next action: {:?}\n- reason: {}\n- required verifier after mutation: {}\n\nDo not emit multiple mutating actions. If no actionable failure is scoped, refresh diagnostics instead of repairing.",
        stderr.trim(),
        directive.decision.phase,
        directive.decision.action,
        directive.decision.reason,
        directive.verifier_command.as_deref().unwrap_or("cargo check"),
    );

    let tlog_path = tlog_path.unwrap_or_else(|| resolve_tlog_path(Some(workspace.as_path()), Some("CANON_TLOG_PATH")));
    let request = RequestDispatch {
        dispatch_id: Uuid::new_v4().to_string(),
        parent_request_id: "harness_repair_driver".to_string(),
        agent_id: "exec".to_string(),
        task_prompt: prompt,
        task_kind: "harness_repair".to_string(),
        deps: Vec::new(),
        workspace_scope: Some(workspace.display().to_string()),
        dispatched: true,
    };
    let meta = CanonPayloadMeta {
        file: file!().to_string(),
        line: line!(),
    };
    let _ = write_shaped_event_auto(
        &tlog_path,
        "harness_repair_driver",
        EventKind::RequestDispatch,
        &request,
        Vec::new(),
        true,
        meta,
    )?;
    eprintln!(
        "[canon-harness-repair] emitted request_dispatch for {crate_name}::{test_name} into {}",
        tlog_path.display()
    );
    Ok(())
}

fn run_target_test(workspace: &Path, crate_name: &str, test_name: &str, always_dispatch: bool) -> Result<String> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("-p")
        .arg(crate_name)
        .arg(test_name)
        .arg("--")
        .arg("--nocapture")
        .current_dir(workspace)
        .output()?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if output.status.success() && !always_dispatch {
        return Err(anyhow!("target test passed; no harness repair requested"));
    }
    if output.status.success() && always_dispatch {
        return Ok(format!(
            "target test passed\ncrate: {crate_name}\ntest: {test_name}\nmode: always-dispatch\ninstruction: inspect harness state, validate the constrained loop, and choose the next verification or repair step if needed"
        ));
    }
    Ok(combined)
}
