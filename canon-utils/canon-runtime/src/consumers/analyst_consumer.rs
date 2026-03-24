use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, RuntimeEvent, LlmCall, CapabilityResult};
use std::io::Write as _;
use std::path::PathBuf;
use uuid::Uuid;

/// Number of ticks without a LoopRewarded before the analyst fires.
const STAGNANT_THRESHOLD: u64 = 20;
/// After the analyst finishes a report, suppress re-firing for this many ticks.
const COOLDOWN_TICKS: u64 = 50;
/// Maximum LLM turns per analysis session.
const MAX_TURNS: usize = 6;

const ANALYST_ROLE: &str = "analyst";
const ANALYST_AGENT_ID: &str = "analyst_chatgpt";
const REPORTS_DIR: &str = "/workspace/ai_sandbox/canon/state/reports_out/analyst";

const SYSTEM_PROMPT: &str = r#"You are a systems analyst for the Canon multi-agent Rust runtime.

You have read-only access to the system's event log (tlog) via Python.
Use Python code blocks to query and analyse the log. When you have enough
information, write a clear diagnosis and actionable recommendations.

## Tools available to you

To run Python code, output a fenced block exactly like this:

```python
import json, os
tlog = os.environ["CANON_TLOG"]
# ... your analysis code ...
print(result)
```

Rules:
- One Python block per turn. Wait for the result before continuing.
- When you have enough information, write your final analysis with NO python block.
- Keep code focused: extract specific metrics, counts, timings, or error patterns.
- The tlog is newline-delimited JSON. Each line is one event.
- Useful event kinds: LoopObserved, RouteSelected, LoopPlanned, LoopActed,
  LoopVerified, LoopRewarded, CapabilityCompleted, CapabilityFailed, ErrorOccurred.
"#;

enum State {
    Idle { ticks_since_reward: u64, cooldown_ticks: u64 },
    PendingLlm { request_id: String, turn: usize, conversation: Vec<String> },
}

pub struct AnalystConsumer {
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    state: State,
}

impl AnalystConsumer {
    pub fn new(tlog_path: PathBuf) -> Self {
        Self { tlog_path, emitter: None, state: State::Idle { ticks_since_reward: 0, cooldown_ticks: 0 } }
    }

    fn tlog_str(&self) -> String {
        self.tlog_path.to_string_lossy().into_owned()
    }

    fn start_session(&mut self, question: &str) {
        let Some(emitter) = &self.emitter else { return; };
        let summary = tlog_summarise(&self.tlog_str()).unwrap_or_else(|e| format!("(tlog read error: {e})"));
        let first_prompt = format!("{SYSTEM_PROMPT}\n\n---\n\n## Question\n{question}\n\n---\n\n{summary}");
        let request_id = Uuid::new_v4().to_string();
        emitter.emit(RuntimeEvent::Llm(LlmCall {
            request_id: request_id.clone(),
            prompt: first_prompt.clone(),
            role: Some(ANALYST_ROLE.to_string()),
            agent_id: Some(ANALYST_AGENT_ID.to_string()),
        }));
        self.state = State::PendingLlm { request_id, turn: 1, conversation: vec![first_prompt] };
    }

    fn continue_session(&mut self, llm_response: String, code: String, turn: usize, mut conversation: Vec<String>) {
        let Some(emitter) = &self.emitter else {
            self.state = State::Idle { ticks_since_reward: 0, cooldown_ticks: 0 };
            return;
        };
        let tlog = self.tlog_str();
        let result_block = match python_run(&code, &tlog) {
            Ok(r) => r.to_context_block(),
            Err(e) => format!("error running python: {e}"),
        };
        conversation.push(llm_response);
        conversation.push(format!("## Python result\n```\n{result_block}\n```"));
        let prompt = conversation.join("\n\n---\n\n");
        let request_id = Uuid::new_v4().to_string();
        emitter.emit(RuntimeEvent::Llm(LlmCall {
            request_id: request_id.clone(),
            prompt,
            role: Some(ANALYST_ROLE.to_string()),
            agent_id: Some(ANALYST_AGENT_ID.to_string()),
        }));
        self.state = State::PendingLlm { request_id, turn: turn + 1, conversation };
    }

    fn finish_session(&mut self, report: String) {
        write_report(&report);
        self.state = State::Idle { ticks_since_reward: 0, cooldown_ticks: COOLDOWN_TICKS };
    }
}

impl EventConsumer for AnalystConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::LoopRewarded(_) => {
                if let State::Idle { ticks_since_reward, cooldown_ticks } = &mut self.state {
                    *ticks_since_reward = 0;
                    *cooldown_ticks = cooldown_ticks.saturating_sub(1);
                }
            }
            RuntimeEvent::Tick(_) => {
                let (tsr, cooldown) = match &mut self.state {
                    State::Idle { ticks_since_reward, cooldown_ticks } => (ticks_since_reward, cooldown_ticks),
                    _ => return,
                };
                if *cooldown > 0 {
                    *cooldown -= 1;
                    return;
                }
                *tsr += 1;
                if *tsr >= STAGNANT_THRESHOLD {
                    *tsr = 0;
                    self.start_session("The system appears stagnant. Diagnose why progress has halted and suggest fixes.");
                }
            }
            RuntimeEvent::CapabilityCompleted(done) => {
                if done.capability != "llm.call" { return; }
                let (expected_id, turn, conversation) = match &self.state {
                    State::PendingLlm { request_id, turn, conversation } => (request_id.clone(), *turn, conversation.clone()),
                    _ => return,
                };
                if done.request_id != expected_id { return; }
                let response_text = match &done.result {
                    CapabilityResult::Llm(res) => res.response.get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| res.response.as_str().unwrap_or(""))
                        .to_string(),
                    _ => return,
                };
                if turn >= MAX_TURNS {
                    self.finish_session(response_text);
                    return;
                }
                match extract_python_block(&response_text) {
                    Some(code) => self.continue_session(response_text, code, turn, conversation),
                    None => self.finish_session(response_text),
                }
            }
            RuntimeEvent::CapabilityFailed(fail) => {
                if let State::PendingLlm { request_id, .. } = &self.state {
                    if fail.request_id == *request_id {
                        eprintln!("[analyst_consumer] LLM capability failed: {:?}", fail.error);
                        self.state = State::Idle { ticks_since_reward: 0, cooldown_ticks: COOLDOWN_TICKS };
                    }
                }
            }
            _ => {}
        }
    }
}

fn extract_python_block(text: &str) -> Option<String> {
    let start = text.find("```python")?;
    let after = &text[start + 9..];
    let end = after.find("```")?;
    let code = after[..end].trim().to_string();
    if code.is_empty() { None } else { Some(code) }
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
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(code.as_bytes())?;
    tmp.flush()?;
    let output = Command::new("python3")
        .arg(tmp.path())
        .env("CANON_TLOG", tlog_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    Ok(PythonResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

fn tlog_summarise(path: &str) -> anyhow::Result<String> {
    use std::collections::BTreeMap;
    // Support both single-file and segmented (directory) tlogs.
    let lines: Vec<String> = if std::path::Path::new(path).is_dir() {
        let events = canon_event_store::read_any_events_from_path(std::path::Path::new(path))?;
        events
            .iter()
            .filter_map(|e| match e {
                canon_event_store::AnyEvent::Canon(c) => serde_json::to_string(c).ok(),
                canon_event_store::AnyEvent::Code(r) => serde_json::to_string(r).ok(),
            })
            .collect()
    } else {
        std::fs::read_to_string(path)?.lines().map(|s| s.to_string()).collect()
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_verified: Option<String> = None;
    let mut last_planned: Option<String> = None;
    let mut last_rewarded: Option<String> = None;
    for line in lines.iter() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let kind = v["kind"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(kind.clone()).or_insert(0) += 1;
        match kind.as_str() {
            "LoopVerified" => last_verified = Some(line.to_string()),
            "LoopPlanned" => last_planned = Some(line.to_string()),
            "LoopRewarded" => last_rewarded = Some(line.to_string()),
            _ => {}
        }
    }
    let total = lines.len();
    let mut out = format!("## Tlog summary ({total} events)\n\n### Event counts\n");
    for (k, n) in &counts { out.push_str(&format!("- {k}: {n}\n")); }
    out.push_str("\n### Most recent key events\n");
    for (label, ev) in [("LoopVerified", &last_verified), ("LoopPlanned", &last_planned), ("LoopRewarded", &last_rewarded)] {
        match ev {
            Some(s) => out.push_str(&format!("\n**{label}**\n```json\n{s}\n```\n")),
            None => out.push_str(&format!("\n**{label}**: (none)\n")),
        }
    }
    out.push_str("\n### Last 40 raw events\n```\n");
    for line in lines.iter().rev().take(40).rev() {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("```\n");
    Ok(out)
}
