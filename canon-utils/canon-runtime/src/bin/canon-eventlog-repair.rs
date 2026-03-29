use anyhow::{anyhow, bail, Context, Result};
use canon_event_store::{read_any_events_from_path, AnyEvent};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_WORKSPACE: &str = "/workspace/ai_sandbox/canon";
const DEFAULT_MAX_STEPS: usize = 30;
const DEFAULT_MAX_EVENTS: usize = 200;

#[derive(Debug)]
struct Args {
    workspace: PathBuf,
    crate_name: String,
    test_name: String,
    event_jsonl: PathBuf,
    event_tlog: Option<PathBuf>,
    max_steps: usize,
    max_events: usize,
    dry_run: bool,
}

#[derive(Clone, Debug, Default)]
struct EventRecord {
    kind: String,
    actor: String,
    message: Option<String>,
    status: Option<String>,
    debug_kind: Option<String>,
    approved_route: Option<String>,
    gate_rules_fired: Vec<String>,
    meta_file: Option<String>,
    meta_line: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncidentKind {
    LlmTimeoutPlanLoop,
    ObserveSuppressedByPendingSuccessor,
    RepeatedDeterministicPlanWithoutRecovery,
    RepeatedRouteSelectedBeforePlanningCompleted,
    GenericEventLogFailure,
}

impl IncidentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::LlmTimeoutPlanLoop => "llm_timeout_plan_loop",
            Self::ObserveSuppressedByPendingSuccessor => {
                "observe_suppressed_by_pending_successor"
            }
            Self::RepeatedDeterministicPlanWithoutRecovery => {
                "repeated_deterministic_plan_without_recovery"
            }
            Self::RepeatedRouteSelectedBeforePlanningCompleted => {
                "repeated_route_selected_before_planning_completed"
            }
            Self::GenericEventLogFailure => "generic_event_log_failure",
        }
    }
}

#[derive(Debug)]
struct IncidentReport {
    incident: IncidentKind,
    summary: String,
    guidance: String,
    files: Vec<String>,
    lines: Vec<String>,
    event_excerpt: String,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let records = load_event_records(&args)?;
    if records.is_empty() {
        bail!("no event records were parsed from the provided event source");
    }

    let report = classify_incident(&records);
    let synthesized_failure = build_failure_output(&args, &report);
    let synthetic_test = synthetic_test_name(report.incident);

    if args.dry_run {
        println!("{synthesized_failure}");
        return Ok(());
    }

    let state_dir = args.workspace.join("state");
    fs::create_dir_all(&state_dir)?;
    let failure_path = state_dir.join("eventlog_repair_failure.txt");
    fs::write(&failure_path, &synthesized_failure)?;

    rebuild_harness_repair(&args.workspace)?;
    let mut cmd = Command::new(args.workspace.join("target/debug/canon-harness-repair"));
    cmd.arg(&args.crate_name)
        .arg(synthetic_test)
        .arg("--workspace")
        .arg(&args.workspace)
        .arg("--stderr-file")
        .arg(&failure_path)
        .arg("--max-steps")
        .arg(args.max_steps.to_string())
        .arg("--incident-file")
        .arg(&failure_path);

    let status = cmd
        .status()
        .with_context(|| "failed to run canon-harness-repair from canon-eventlog-repair")?;

    if status.success() {
        Ok(())
    } else {
        bail!(
            "canon-harness-repair exited non-zero for {}::{} with status {}",
            args.crate_name,
            synthetic_test,
            status
        );
    }
}

fn synthetic_test_name(incident: IncidentKind) -> &'static str {
    match incident {
        IncidentKind::LlmTimeoutPlanLoop => {
            "synthetic_llm_timeout_plan_loop_incident"
        }
        IncidentKind::ObserveSuppressedByPendingSuccessor => {
            "synthetic_observe_suppressed_pending_successor_incident"
        }
        IncidentKind::RepeatedDeterministicPlanWithoutRecovery => {
            "synthetic_repeated_deterministic_plan_without_recovery_incident"
        }
        IncidentKind::RepeatedRouteSelectedBeforePlanningCompleted => {
            "synthetic_repeated_route_selected_before_planning_completed_incident"
        }
        IncidentKind::GenericEventLogFailure => {
            "synthetic_generic_event_trigger_incident"
        }
    }
}

fn rebuild_harness_repair(workspace: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("canon-runtime")
        .arg("--bin")
        .arg("canon-harness-repair")
        .current_dir(workspace)
        .status()
        .context("failed to rebuild canon-harness-repair")?;
    if status.success() {
        Ok(())
    } else {
        bail!("rebuilding canon-harness-repair failed with status {}", status);
    }
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let mut workspace = PathBuf::from(DEFAULT_WORKSPACE);
    let mut crate_name: Option<String> = None;
    let mut test_name: Option<String> = None;
    let mut event_jsonl: Option<PathBuf> = None;
    let mut event_tlog: Option<PathBuf> = None;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut max_events = DEFAULT_MAX_EVENTS;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                workspace = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("missing value for --workspace"))?,
                );
            }
            "--crate" => {
                crate_name = Some(
                    args.next()
                        .ok_or_else(|| anyhow!("missing value for --crate"))?,
                );
            }
            "--test" => {
                test_name = Some(
                    args.next()
                        .ok_or_else(|| anyhow!("missing value for --test"))?,
                );
            }
            "--event-jsonl" => {
                event_jsonl = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("missing value for --event-jsonl"))?,
                ));
            }
            "--event-tlog" => {
                event_tlog = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow!("missing value for --event-tlog"))?,
                ));
            }
            "--max-steps" => {
                max_steps = args
                    .next()
                    .ok_or_else(|| anyhow!("missing value for --max-steps"))?
                    .parse()
                    .context("--max-steps must be an integer")?;
            }
            "--max-events" => {
                max_events = args
                    .next()
                    .ok_or_else(|| anyhow!("missing value for --max-events"))?
                    .parse()
                    .context("--max-events must be an integer")?;
            }
            "--dry-run" => dry_run = true,
            other => bail!("unknown argument: {other}"),
        }
    }

    Ok(Args {
        workspace,
        crate_name: crate_name.ok_or_else(|| anyhow!("missing --crate"))?,
        test_name: test_name.ok_or_else(|| anyhow!("missing --test"))?,
        event_jsonl: event_jsonl.unwrap_or_else(|| {
            PathBuf::from("/workspace/ai_sandbox/canon/state/event_window.jsonl")
        }),
        event_tlog,
        max_steps,
        max_events,
        dry_run,
    })
}

fn load_event_records(args: &Args) -> Result<Vec<EventRecord>> {
    if let Some(tlog_path) = &args.event_tlog {
        return parse_event_records_from_tlog(tlog_path, args.max_events);
    }

    if args.event_jsonl.exists() {
        let raw = fs::read_to_string(&args.event_jsonl)
            .with_context(|| format!("failed to read {}", args.event_jsonl.display()))?;
        return parse_event_records(&raw, args.max_events);
    }

    let default_tlog = args.workspace.join("state/event_log/event.tlog.d");
    if default_tlog.exists() {
        return parse_event_records_from_tlog(&default_tlog, args.max_events);
    }

    Err(anyhow!(
        "no event source found; provide --event-tlog or --event-jsonl"
    ))
}

fn parse_event_records(raw: &str, max_events: usize) -> Result<Vec<EventRecord>> {
    let mut out = Vec::new();
    for line in raw.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed)
            .with_context(|| format!("invalid jsonl event line: {trimmed}"))?;
        out.push(EventRecord::from_value(&value));
        if out.len() >= max_events {
            break;
        }
    }
    out.reverse();
    Ok(out)
}

fn parse_event_records_from_tlog(path: &Path, max_events: usize) -> Result<Vec<EventRecord>> {
    let events = read_any_events_from_path(path)
        .with_context(|| format!("failed to read tlog {}", path.display()))?;
    let mut out = Vec::new();

    for event in events.into_iter().rev() {
        out.push(EventRecord::from_any_event(&event));
        if out.len() >= max_events {
            break;
        }
    }

    out.reverse();
    Ok(out)
}

impl EventRecord {
    fn from_value(value: &Value) -> Self {
        let payload = value.get("payload").unwrap_or(&Value::Null);
        let data = payload.get("data").unwrap_or(&Value::Null);
        let meta = payload.get("meta").unwrap_or(&Value::Null);

        Self {
            kind: value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            actor: value
                .get("actor")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            message: data
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            status: data
                .get("status")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            debug_kind: data
                .get("kind")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            approved_route: data
                .get("approved_route")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            gate_rules_fired: data
                .get("gate_rules_fired")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            meta_file: meta
                .get("file")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            meta_line: meta.get("line").and_then(Value::as_u64),
        }
    }

    fn from_any_event(event: &AnyEvent) -> Self {
        let text = format!("{event:?}");
        let lowered = text.to_lowercase();

        let kind = if lowered.contains("capabilityfailed") || lowered.contains("capability_failed") {
            "capability_failed"
        } else if lowered.contains("planningcompleted") || lowered.contains("planning_completed") {
            "planning_completed"
        } else if lowered.contains("routeselected") || lowered.contains("route_selected") {
            "route_selected"
        } else if lowered.contains("erroroccurred") || lowered.contains("error_occurred") {
            "error_occurred"
        } else if lowered.contains("debugevent") || lowered.contains("debug") {
            "debug"
        } else {
            "unknown"
        }
        .to_string();

        let actor = if lowered.contains("loop_stage_executor") {
            "loop_stage_executor".to_string()
        } else if lowered.contains("llm_executor") {
            "llm_executor".to_string()
        } else if lowered.contains("event-runtime") || lowered.contains("event_runtime") {
            "event-runtime".to_string()
        } else if lowered.contains("supervisor") {
            "supervisor".to_string()
        } else if lowered.contains("actor: \"plan\"") || lowered.contains("actor:\"plan\"") {
            "plan".to_string()
        } else {
            String::new()
        };

        let message = extract_debug_string(&text, "message: \"").or_else(|| {
            if lowered.contains("llm call timed out") {
                Some("llm call timed out".to_string())
            } else {
                None
            }
        });

        let status = if lowered.contains("status: \"llm_failed\"") || lowered.contains("status:\"llm_failed\"") {
            Some("llm_failed".to_string())
        } else {
            None
        };

        let debug_kind = if lowered.contains("observe_suppressed_due_to_pending_successor") {
            Some("observe_suppressed_due_to_pending_successor".to_string())
        } else {
            None
        };

        let approved_route = if lowered.contains("approved_route: \"plan\"")
            || lowered.contains("approved_route:\"plan\"")
            || lowered.contains("suggested_route: \"plan\"")
            || lowered.contains("suggested_route:\"plan\"")
        {
            Some("plan".to_string())
        } else {
            None
        };

        let mut gate_rules_fired = Vec::new();
        if lowered.contains("missing_target_plan") {
            gate_rules_fired.push("deterministic:missing_target_plan".to_string());
        }

        Self {
            kind,
            actor,
            message,
            status,
            debug_kind,
            approved_route,
            gate_rules_fired,
            meta_file: extract_meta_string(&text, "file: \""),
            meta_line: extract_meta_u64(&text, "line: "),
        }
    }
}

fn extract_meta_string(text: &str, key: &str) -> Option<String> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_meta_u64(text: &str, key: &str) -> Option<u64> {
    let start = text.find(key)? + key.len();
    let digits: String = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn extract_debug_string(text: &str, key: &str) -> Option<String> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn classify_incident(records: &[EventRecord]) -> IncidentReport {
    let llm_timeout = records.iter().any(|record| {
        record.kind == "capability_failed"
            && record.message.as_deref() == Some("llm call timed out")
    });
    let planning_failed = records.iter().any(|record| {
        record.kind == "planning_completed" && record.status.as_deref() == Some("llm_failed")
    });
    let observe_suppressed = records.iter().any(|record| {
        record.kind == "debug"
            && record.debug_kind.as_deref()
                == Some("observe_suppressed_due_to_pending_successor")
    });
    let deterministic_plan = records.iter().any(|record| {
        record.kind == "route_selected"
            && record.approved_route.as_deref() == Some("plan")
            && record
                .gate_rules_fired
                .iter()
                .any(|rule| rule.contains("missing_target_plan"))
    });
    let repeated_route_selected_before_planning_completed = records.iter().any(|record| {
        record.kind == "error_occurred"
            && record.message.as_deref().is_some_and(|message| {
                message.contains("expected=planning_completed; got=route_selected")
            })
    });

    let incident = if llm_timeout && planning_failed && observe_suppressed && deterministic_plan {
        IncidentKind::LlmTimeoutPlanLoop
    } else if repeated_route_selected_before_planning_completed {
        IncidentKind::RepeatedRouteSelectedBeforePlanningCompleted
    } else if observe_suppressed {
        IncidentKind::ObserveSuppressedByPendingSuccessor
    } else if deterministic_plan && planning_failed {
        IncidentKind::RepeatedDeterministicPlanWithoutRecovery
    } else {
        IncidentKind::GenericEventLogFailure
    };

    let files = collect_files(records);
    let lines = collect_file_lines(records);
    let event_excerpt = records
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(format_record_excerpt)
        .collect::<Vec<_>>()
        .join("\n");

    let (summary, guidance) = match incident {
        IncidentKind::LlmTimeoutPlanLoop => (
            "LLM timeout closed planning, but control recovery stayed blocked behind a pending planning_completed successor while routing immediately re-issued deterministic plan.".to_string(),
            [
                "Target the control-successor / recovery path, not only the planner payload parser.",
                "Preserve planning_completed emission, but ensure timeout/failure closes or rewrites the pending successor contract so observe recovery can run.",
                "Inspect canon-utils/canon-loop/src/executor.rs for observe suppression and successor clearing logic.",
                "Inspect canon-utils/canon-route/src/executor.rs for immediate deterministic re-plan after llm_failed.",
                "Inspect canon-utils/canon-exec/src/exec/llm.rs only if timeout semantics need to emit a distinct failure class or recovery signal.",
                "Add or update a synthetic harness test that reproduces: llm timeout -> planning_completed(llm_failed) -> observe suppressed -> repeated deterministic plan.",
            ]
            .join("\n"),
        ),
        IncidentKind::ObserveSuppressedByPendingSuccessor => (
            "Observe recovery is being suppressed by a stale or overly strict pending successor contract.".to_string(),
            [
                "Inspect loop executor successor gating first.",
                "Verify that failure recovery events can clear or rewrite pending_required_successor safely.",
                "Add a synthetic harness case for suppressed observe after error_occurred.",
            ]
            .join("\n"),
        ),
        IncidentKind::RepeatedDeterministicPlanWithoutRecovery => (
            "Deterministic plan routing repeats after failure without a successful recovery transition.".to_string(),
            [
                "Inspect route executor for repeated deterministic plan replay.",
                "Verify plan failure transitions allow observe or a fresh recovery route before re-planning.",
                "Add a synthetic harness case for repeated deterministic plan without recovery.",
            ]
            .join("\n"),
        ),
        IncidentKind::RepeatedRouteSelectedBeforePlanningCompleted => (
            "Control routing is re-emitting route_selected(plan) before the required planning_completed successor is recorded.".to_string(),
            [
                "Inspect route executor / dispatch handoff for duplicate plan-route emission while a planning_completed successor is still pending.",
                "Verify awaiting_control_successor or pending_required_successor suppresses repeated route_selected(plan).",
                "Add a synthetic harness case that reproduces: route_selected(plan) -> route_selected(plan) before planning_completed.",
            ]
            .join("\n"),
        ),
        IncidentKind::GenericEventLogFailure => (
            "Generic event-log failure pattern detected.".to_string(),
            [
                "Read the files cited in the event meta first.",
                "Prefer the smallest fix that restores a valid control transition.",
                "Add a synthetic reproduction for the observed event sequence.",
            ]
            .join("\n"),
        ),
    };

    IncidentReport {
        incident,
        summary,
        guidance,
        files,
        lines,
        event_excerpt,
    }
}

fn collect_files(records: &[EventRecord]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for record in records {
        if let Some(file) = &record.meta_file {
            set.insert(file.clone());
        }
    }
    set.into_iter().collect()
}

fn collect_file_lines(records: &[EventRecord]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for record in records {
        if let (Some(file), Some(line)) = (&record.meta_file, record.meta_line) {
            set.insert(format!("{file}:{line}"));
        }
    }
    set.into_iter().collect()
}

fn format_record_excerpt(record: &EventRecord) -> String {
    let mut parts = vec![format!("kind={}", record.kind)];
    if !record.actor.is_empty() {
        parts.push(format!("actor={}", record.actor));
    }
    if let Some(message) = &record.message {
        parts.push(format!("message={message}"));
    }
    if let Some(status) = &record.status {
        parts.push(format!("status={status}"));
    }
    if let Some(debug_kind) = &record.debug_kind {
        parts.push(format!("debug_kind={debug_kind}"));
    }
    if let Some(route) = &record.approved_route {
        parts.push(format!("approved_route={route}"));
    }
    if let Some(file) = &record.meta_file {
        if let Some(line) = record.meta_line {
            parts.push(format!("at={file}:{line}"));
        } else {
            parts.push(format!("at={file}"));
        }
    }
    parts.join(" ")
}

fn build_failure_output(args: &Args, report: &IncidentReport) -> String {
    let files = if report.files.is_empty() {
        "<none>".to_string()
    } else {
        report.files.join("\n")
    };
    let lines = if report.lines.is_empty() {
        "<none>".to_string()
    } else {
        report.lines.join("\n")
    };

    format!(
        "EVENT-LOG REPAIR INCIDENT\n\
incident_kind={incident}\n\
crate={crate_name}\n\
test={test_name}\n\
\n\
Summary:\n\
{summary}\n\
\n\
Guidance:\n\
{guidance}\n\
\n\
Likely files:\n\
{files}\n\
\n\
Likely file:line anchors:\n\
{lines}\n\
\n\
Recent event excerpt:\n\
{event_excerpt}\n\
\n\
Repair requirements:\n\
- Fix the control-path bug surfaced by the event log.\n\
- Add or update a synthetic harness / regression test for this exact incident shape.\n\
- Do not paper over the issue by merely suppressing error output.\n\
- Prefer the smallest change that restores valid successor clearing and recovery routing.\n",
        incident = report.incident.as_str(),
        crate_name = args.crate_name,
        test_name = args.test_name,
        summary = report.summary,
        guidance = report.guidance,
        files = files,
        lines = lines,
        event_excerpt = report.event_excerpt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        kind: &str,
        actor: &str,
        message: Option<&str>,
        status: Option<&str>,
        debug_kind: Option<&str>,
        approved_route: Option<&str>,
        gate_rules_fired: &[&str],
        meta_file: Option<&str>,
        meta_line: Option<u64>,
    ) -> EventRecord {
        EventRecord {
            kind: kind.to_string(),
            actor: actor.to_string(),
            message: message.map(ToString::to_string),
            status: status.map(ToString::to_string),
            debug_kind: debug_kind.map(ToString::to_string),
            approved_route: approved_route.map(ToString::to_string),
            gate_rules_fired: gate_rules_fired.iter().map(|v| (*v).to_string()).collect(),
            meta_file: meta_file.map(ToString::to_string),
            meta_line,
        }
    }

    #[test]
    fn classify_incident_detects_llm_timeout_plan_loop() {
        let records = vec![
            record(
                "capability_failed",
                "event-runtime",
                Some("llm call timed out"),
                None,
                None,
                None,
                &[],
                Some("canon-utils/canon-exec/src/exec/llm.rs"),
                Some(355),
            ),
            record(
                "debug",
                "loop_stage_executor",
                None,
                None,
                Some("observe_suppressed_due_to_pending_successor"),
                None,
                &[],
                Some("canon-utils/canon-loop/src/executor.rs"),
                Some(91),
            ),
            record(
                "planning_completed",
                "plan",
                None,
                Some("llm_failed"),
                None,
                None,
                &[],
                Some("canon-utils/canon-loop/src/executor.rs"),
                Some(296),
            ),
            record(
                "route_selected",
                "supervisor",
                None,
                None,
                None,
                Some("plan"),
                &["deterministic:missing_target_plan"],
                Some("canon-utils/canon-route/src/executor.rs"),
                Some(796),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(report.incident, IncidentKind::LlmTimeoutPlanLoop);
        assert!(report.guidance.contains("successor"));
    }

    #[test]
    fn build_failure_output_includes_incident_and_files() {
        let args = Args {
            workspace: PathBuf::from("/workspace/ai_sandbox/canon"),
            crate_name: "canon-loop".to_string(),
            test_name: "synthetic_timeout_recovery".to_string(),
            event_jsonl: PathBuf::from("/tmp/example.jsonl"),
            event_tlog: None,
            max_steps: 8,
            max_events: 200,
            dry_run: false,
        };

        let report = IncidentReport {
            incident: IncidentKind::ObserveSuppressedByPendingSuccessor,
            summary: "Observe recovery is blocked.".to_string(),
            guidance: "Inspect loop executor successor gating first.".to_string(),
            files: vec![
                "canon-utils/canon-loop/src/executor.rs".to_string(),
                "canon-utils/canon-route/src/executor.rs".to_string(),
            ],
            lines: vec![
                "canon-utils/canon-loop/src/executor.rs:91".to_string(),
                "canon-utils/canon-route/src/executor.rs:796".to_string(),
            ],
            event_excerpt: "kind=debug actor=loop_stage_executor".to_string(),
        };

        let output = build_failure_output(&args, &report);
        assert!(output.contains("incident_kind=observe_suppressed_by_pending_successor"));
        assert!(output.contains("canon-utils/canon-loop/src/executor.rs"));
        assert!(output.contains("synthetic_timeout_recovery"));
    }

    #[test]
    fn synthetic_llm_timeout_plan_loop_incident() {
        let records = vec![
            record(
                "capability_failed",
                "event-runtime",
                Some("llm call timed out"),
                None,
                None,
                None,
                &[],
                Some("canon-utils/canon-exec/src/exec/llm.rs"),
                Some(373),
            ),
            record(
                "planning_completed",
                "plan",
                None,
                Some("llm_failed"),
                None,
                None,
                &[],
                Some("canon-utils/canon-loop/src/stage/plan.rs"),
                Some(1038),
            ),
            record(
                "debug",
                "loop_stage_executor",
                None,
                None,
                Some("observe_suppressed_due_to_pending_successor"),
                None,
                &[],
                Some("canon-utils/canon-loop/src/executor.rs"),
                Some(91),
            ),
            record(
                "route_selected",
                "supervisor",
                None,
                None,
                None,
                Some("plan"),
                &["deterministic:missing_target_plan"],
                Some("canon-utils/canon-route/src/executor.rs"),
                Some(796),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(report.incident, IncidentKind::LlmTimeoutPlanLoop);
        assert_eq!(
            synthetic_test_name(report.incident),
            "synthetic_llm_timeout_plan_loop_incident"
        );
        assert!(report.summary.contains("LLM timeout"));
        assert!(report.guidance.contains("observe recovery"));
    }

    #[test]
    fn synthetic_observe_suppressed_pending_successor_incident() {
        let records = vec![
            record(
                "debug",
                "loop_stage_executor",
                None,
                None,
                Some("observe_suppressed_due_to_pending_successor"),
                None,
                &[],
                Some("canon-utils/canon-loop/src/executor.rs"),
                Some(91),
            ),
            record(
                "error_occurred",
                "event-runtime",
                Some("stale pending successor blocked observe"),
                None,
                None,
                None,
                &[],
                Some("canon-utils/canon-runtime/src/bus.rs"),
                Some(239),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(report.incident, IncidentKind::ObserveSuppressedByPendingSuccessor);
        assert_eq!(
            synthetic_test_name(report.incident),
            "synthetic_observe_suppressed_pending_successor_incident"
        );
        assert!(report.summary.contains("Observe recovery"));
        assert!(report.guidance.contains("successor gating"));
    }

    #[test]
    fn synthetic_repeated_deterministic_plan_without_recovery_incident() {
        let records = vec![
            record(
                "planning_completed",
                "plan",
                None,
                Some("llm_failed"),
                None,
                None,
                &[],
                Some("canon-utils/canon-loop/src/stage/plan.rs"),
                Some(1038),
            ),
            record(
                "route_selected",
                "supervisor",
                None,
                None,
                None,
                Some("plan"),
                &["deterministic:missing_target_plan"],
                Some("canon-utils/canon-route/src/executor.rs"),
                Some(796),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(
            report.incident,
            IncidentKind::RepeatedDeterministicPlanWithoutRecovery
        );
        assert_eq!(
            synthetic_test_name(report.incident),
            "synthetic_repeated_deterministic_plan_without_recovery_incident"
        );
        assert!(report.summary.contains("Deterministic plan routing repeats"));
        assert!(report.guidance.contains("fresh recovery route"));
    }

    #[test]
    fn synthetic_repeated_route_selected_before_planning_completed_incident() {
        let records = vec![
            record(
                "route_selected",
                "supervisor",
                None,
                None,
                None,
                Some("plan"),
                &["deterministic:missing_target_plan"],
                Some("canon-utils/canon-route/src/executor.rs"),
                Some(809),
            ),
            record(
                "error_occurred",
                "event-runtime",
                Some("invariant violation: missing required successor after route_selected id=abc; expected=planning_completed; got=route_selected; note=approved_route=plan"),
                None,
                None,
                None,
                &[],
                Some("canon-utils/canon-runtime-events/src/tlog/binary.rs"),
                Some(383),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(
            report.incident,
            IncidentKind::RepeatedRouteSelectedBeforePlanningCompleted
        );
        assert_eq!(
            synthetic_test_name(report.incident),
            "synthetic_repeated_route_selected_before_planning_completed_incident"
        );
        assert!(report.summary.contains("route_selected(plan)"));
        assert!(report.guidance.contains("planning_completed"));
    }

    #[test]
    fn synthetic_generic_event_trigger_incident() {
        let records = vec![
            record(
                "debug",
                "event-runtime",
                Some("generic bad event"),
                None,
                None,
                None,
                &[],
                Some("canon-utils/canon-runtime-events/src/tlog/binary.rs"),
                Some(383),
            ),
        ];

        let report = classify_incident(&records);
        assert_eq!(report.incident, IncidentKind::GenericEventLogFailure);
        assert_eq!(
            synthetic_test_name(report.incident),
            "synthetic_generic_event_trigger_incident"
        );
        assert!(report.summary.contains("Generic event-log failure"));
        assert!(report.guidance.contains("smallest fix"));
    }
}
