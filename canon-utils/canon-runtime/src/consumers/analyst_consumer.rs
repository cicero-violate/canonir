use canon_event::{CapabilityResult, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, LlmCall, RuntimeEvent};
use canon_proc_macros::must_emit;
use canon_skills::global_registry;
use std::io::Write as _;
use std::path::PathBuf;
use uuid::Uuid;

/// Number of ticks without a LoopRewarded before the analyst fires.
const STAGNANT_THRESHOLD: u64 = 20;
/// After the analyst finishes a report, suppress re-firing for this many ticks.
const COOLDOWN_TICKS: u64 = 50;
/// Maximum LLM turns per analysis session.
const MAX_TURNS: usize = 16;

const ANALYST_ROLE: &str = "analyst";
const ANALYST_AGENT_ID: &str = "analyst_chatgpt";
const REPORTS_DIR: &str = "/workspace/ai_sandbox/canon/state/reports_out/analyst";

enum State {
    Idle { ticks_since_reward: u64, cooldown_ticks: u64 },
    PendingLlm { request_id: String, turn: usize },
}

pub struct AnalystConsumer {
    tlog_path: PathBuf,
    state: State,
}

impl AnalystConsumer {
    pub fn new(tlog_path: PathBuf) -> Self {
        Self { tlog_path, state: State::Idle { ticks_since_reward: 0, cooldown_ticks: 0 } }
    }

    fn tlog_str(&self) -> String {
        self.tlog_path.to_string_lossy().into_owned()
    }

    fn start_session(&mut self, question: &str) -> EventOutcome {
        let first_prompt = match global_registry().load("analyst/full_analysis") {
            Ok(skill) => format!("{}\n\n{question}", skill.prompt),
            Err(e) => {
                eprintln!("[analyst_consumer] failed to load skill: {e}");
                return EventOutcome::NoOp("analyst_skill_load_failed");
            }
        };
        let request_id = Uuid::new_v4().to_string();
        self.state = State::PendingLlm { request_id: request_id.clone(), turn: 1 };
        EventOutcome::emit(
            RuntimeEvent::Llm(LlmCall {
                request_id: request_id.clone(),
                prompt: first_prompt,
                role: Some(ANALYST_ROLE.to_string()),
                agent_id: Some(ANALYST_AGENT_ID.to_string()),
                dispatched: true,
                system: None,
                system_prompt_id: None,
                context_base: None,
                context_base_id: None,
                prompt_base_id: None,
                prev_prompt_id: None,
            }),
            file!(),
            line!(),
        )
    }

    fn continue_session(&mut self, code: String, turn: usize) -> EventOutcome {
        let tlog = self.tlog_str();
        let result_block = match python_run(&code, &tlog) {
            Ok(r) => r.to_context_block(),
            Err(e) => format!("error running python: {e}"),
        };

        let prompt = format!("## Python result\n```\n{result_block}\n```");
        let request_id = Uuid::new_v4().to_string();
        self.state = State::PendingLlm { request_id: request_id.clone(), turn: turn + 1 };
        EventOutcome::emit(
            RuntimeEvent::Llm(LlmCall {
                request_id,
                prompt,
                role: Some(ANALYST_ROLE.to_string()),
                agent_id: Some(ANALYST_AGENT_ID.to_string()),
                dispatched: true,
                system: None,
                system_prompt_id: None,
                context_base: None,
                context_base_id: None,
                prompt_base_id: None,
                prev_prompt_id: None,
            }),
            file!(),
            line!(),
        )
    }

    fn continue_session_no_python(&mut self, turn: usize) -> EventOutcome {
        let nudge = "You skipped mandatory phases. You must not write the final report until you have marked Phase 7 complete. Resume from the next unfinished phase and emit a Python block.";
        let prompt = nudge.to_string();
        let request_id = Uuid::new_v4().to_string();
        self.state = State::PendingLlm { request_id: request_id.clone(), turn: turn + 1 };
        EventOutcome::emit(
            RuntimeEvent::Llm(LlmCall {
                request_id,
                prompt,
                role: Some(ANALYST_ROLE.to_string()),
                agent_id: Some(ANALYST_AGENT_ID.to_string()),
                dispatched: true,
                system: None,
                system_prompt_id: None,
                context_base: None,
                context_base_id: None,
                prompt_base_id: None,
                prev_prompt_id: None,
            }),
            file!(),
            line!(),
        )
    }

    fn finish_session(&mut self, report: String) -> EventOutcome {
        if !report.trim().is_empty() {
            write_report(&report);
        }
        self.state = State::Idle { ticks_since_reward: 0, cooldown_ticks: COOLDOWN_TICKS };
        EventOutcome::NoOp("analyst_finished")
    }
}

impl EventConsumer for AnalystConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "analyst_consumer"
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        match event {
            RuntimeEvent::CapabilityRequested(_) => {
                return EventOutcome::NoOp("analyst_capability_requested");
            }
            RuntimeEvent::LoopRewarded(_) => {
                if let State::Idle { ticks_since_reward, cooldown_ticks } = &mut self.state {
                    *ticks_since_reward = 0;
                    *cooldown_ticks = cooldown_ticks.saturating_sub(1);
                }
                EventOutcome::NoOp("analyst_reward_seen")
            }
            RuntimeEvent::Tick(_) => {
                let (tsr, cooldown) = match &mut self.state {
                    State::Idle { ticks_since_reward, cooldown_ticks } => (ticks_since_reward, cooldown_ticks),
                    _ => return EventOutcome::NoOp("analyst_not_idle"),
                };
                if *cooldown > 0 {
                    *cooldown -= 1;
                    return EventOutcome::NoOp("analyst_cooldown");
                }
                *tsr += 1;
                if *tsr >= STAGNANT_THRESHOLD {
                    *tsr = 0;
                    return self.start_session("The system appears stagnant. Diagnose why progress has halted. DO NOT PROVIDE SOLUTIONS");
                }
                EventOutcome::NoOp("analyst_not_stagnant")
            }
            RuntimeEvent::CapabilityCompleted(done) => {
                if done.capability != "llm.call" {
                    return EventOutcome::NoOp("analyst_non_llm_capability");
                }
                let (expected_id, turn) = match &self.state {
                    State::PendingLlm { request_id, turn } => (request_id.clone(), *turn),
                    _ => return EventOutcome::NoOp("analyst_not_pending"),
                };
                if done.request_id != expected_id {
                    return EventOutcome::NoOp("analyst_request_mismatch");
                }
                let response_text = match &done.result {
                    CapabilityResult::Llm(res) => extract_response_text(&res.response),
                    _ => return EventOutcome::NoOp("analyst_wrong_result_type"),
                };
                if turn >= MAX_TURNS {
                    return self.finish_session(response_text);
                }
                match extract_python_block(&response_text) {
                    Some(code) => return self.continue_session(code, turn),
                    None => {
                        if turn < 5 && !response_text.contains("Completed: Phase 7") {
                            eprintln!("[analyst_consumer] LLM skipped phases at turn {turn}, re-prompting");
                            return self.continue_session_no_python(turn);
                        } else {
                            return self.finish_session(response_text);
                        }
                    }
                }
            }
            RuntimeEvent::CapabilityFailed(fail) => {
                if let State::PendingLlm { request_id, .. } = &self.state {
                    if fail.request_id == *request_id {
                        eprintln!("[analyst_consumer] LLM capability failed: {:?}", fail.error);
                        self.state = State::Idle { ticks_since_reward: 0, cooldown_ticks: COOLDOWN_TICKS };
                        return EventOutcome::NoOp("analyst_capability_failed");
                    }
                }
                EventOutcome::NoOp("analyst_other_capability_failed")
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            => {
                EventOutcome::NoOp("non_actionable_event")
            }
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::AgentRegistered(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::RustcCaptureStarted(_)
            | RuntimeEvent::RustcGraphArtifactWritten(_)
            | RuntimeEvent::RustcCaptureCompleted(_)
            | RuntimeEvent::RustcCaptureFailed(_)
            | RuntimeEvent::VerifierPolicyUpdated(_) => EventOutcome::NoOp("analyst_ignored_event"),
        }
    }
}

fn extract_python_block(text: &str) -> Option<String> {
    let start = text.find("```python")?;
    let after = &text[start + 9..];
    let end = after.find("```")?;
    let code = after[..end].trim().to_string();
    if code.is_empty() {
        None
    } else {
        Some(code)
    }
}

fn extract_response_text(v: &serde_json::Value) -> String {
    if let Some(s) = v.get("text").and_then(|t| t.as_str()) {
        return s.to_string();
    }
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(obj) = v.as_object() {
        for val in obj.values() {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

fn write_report(content: &str) {
    let _ = std::fs::create_dir_all(REPORTS_DIR);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = format!("{REPORTS_DIR}/{now}.md");
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("[analyst_consumer] failed to write report to {path}: {e}");
    }
}

struct PythonResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl PythonResult {
    fn to_context_block(&self) -> String {
        let status = if self.exit_code == 0 { "ok" } else { "error" };
        let mut s = format!("exit={} ({})\n", self.exit_code, status);
        if !self.stdout.is_empty() {
            let out = truncate(&self.stdout, 4000);
            s.push_str(&format!("stdout:\n{out}\n"));
        }
        if !self.stderr.is_empty() {
            let err = truncate(&self.stderr, 1000);
            s.push_str(&format!("stderr:\n{err}\n"));
        }
        s
    }
}

fn python_run(code: &str, tlog_path: &str) -> anyhow::Result<PythonResult> {
    use std::process::{Command, Stdio};

    // If tlog_path is a directory (segmented tlog), flatten all .log files into
    // a single temp file so Python can open it with a normal file handle.
    let flat_tlog;
    let effective_tlog: &str = if std::path::Path::new(tlog_path).is_dir() {
        let mut flat = tempfile::NamedTempFile::new()?;
        let mut entries: Vec<_> = std::fs::read_dir(tlog_path)?.flatten().filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("log")).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let contents = std::fs::read(entry.path())?;
            flat.write_all(&contents)?;
        }
        flat.flush()?;
        flat_tlog = flat;
        flat_tlog.path().to_str().unwrap_or(tlog_path)
    } else {
        tlog_path
    };

    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(code.as_bytes())?;
    tmp.flush()?;
    let output = Command::new("python3").arg(tmp.path()).env("CANON_TLOG", effective_tlog).stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;
    Ok(PythonResult { stdout: String::from_utf8_lossy(&output.stdout).into_owned(), stderr: String::from_utf8_lossy(&output.stderr).into_owned(), exit_code: output.status.code().unwrap_or(-1) })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
