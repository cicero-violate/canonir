use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use canon_event::{RuntimeEvent, CapabilityResult, EventEmitter, EventEmitterHandle, LlmCall};
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use canon_goal::GoalSpec;
use crossbeam_channel as cc;

use crate::context::RouteContext;

pub fn heuristic_route_json(ctx: &RouteContext) -> String {
    let route = if ctx.finish_ready {
        canon_decision::RouteKind::Conclude
    } else if ctx.planned_pending > 0 {
        canon_decision::RouteKind::Act
    } else if ctx.acted_unverified {
        canon_decision::RouteKind::Verify
    } else if ctx.workspace_dirty {
        canon_decision::RouteKind::Act
    } else if ctx.context_ready {
        canon_decision::RouteKind::Plan
    } else {
        canon_decision::RouteKind::Observe
    };
    serde_json::json!({
        "route": route.as_str(),
        "rationale": "heuristic proposal from runtime state",
        "confidence": 0.75,
        "signals": {
            "context_ready": ctx.context_ready,
            "workspace_dirty": ctx.workspace_dirty,
            "planned_pending": ctx.planned_pending,
            "acted_unverified": ctx.acted_unverified,
            "finish_ready": ctx.finish_ready,
        }
    })
    .to_string()
}

struct DirectEventEmitter {
    tx: cc::Sender<RuntimeEvent>,
}

impl EventEmitter for DirectEventEmitter {
    fn emit(&self, event: RuntimeEvent) {
        let _ = self.tx.send(event);
    }
    fn emit_located(&self, event: RuntimeEvent, _file: &'static str, _line: u32) {
        let _ = self.tx.send(event);
    }
}

pub fn request_route_via_llm_call(
    workspace: &Path,
    prompt: String,
    timeout: Duration,
    _last_tool_result: Option<serde_json::Value>,
) -> Result<String> {
    let request_id = format!("route-{}", uuid::Uuid::new_v4());
    let event = RuntimeEvent::Llm(LlmCall { request_id: request_id.clone(), prompt: prompt.clone(), role: Some("router".to_string()) });
    let (tx, rx) = cc::unbounded::<RuntimeEvent>();
    let emitter: EventEmitterHandle = std::sync::Arc::new(DirectEventEmitter { tx });
    let ctx = ExecutionContext { workspace: workspace.to_path_buf(), emitter: emitter.clone() };
    let exec = ExecutableEvent::try_from(event.clone()).map_err(|_| anyhow!("llm.call not executable"))?;
    match exec.execute(ctx)? {
        ExecutionResult::Emit(e) => emitter.emit(e),
        ExecutionResult::EmitMany(evs) => evs.into_iter().for_each(|e| emitter.emit(e)),
        ExecutionResult::Deferred => {}
    }

    let deadline = std::time::Instant::now() + timeout;
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(anyhow!("route llm.call timed out"));
        }
        let remaining = deadline.saturating_duration_since(now);
        let event = rx.recv_timeout(remaining).map_err(|_| anyhow!("route llm.call timed out"))?;
        match event {
            RuntimeEvent::CapabilityCompleted(done) if done.request_id == request_id && done.capability == "llm.call" => match done.result {
                CapabilityResult::Llm(res) => {
                    if let Some(text) = res.response.get("text").and_then(|v| v.as_str()) {
                        return Ok(text.to_string());
                    }
                    return Ok(res.response.to_string());
                }
                CapabilityResult::Process(proc) => return Ok(proc.stdout),
                CapabilityResult::Empty => return Ok(String::new()),
            },
            RuntimeEvent::CapabilityFailed(failed) if failed.request_id == request_id && failed.capability == "llm.call" => {
                return Err(anyhow!("route llm.call failed: {}", failed.error));
            }
            _ => {}
        }
    }
}

fn count_loc(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}

fn extract_loc_requirement(spec: &GoalSpec) -> usize {
    spec.requirements.iter().find_map(|req| {
        if req.to_ascii_lowercase().contains("loc") {
            let digits: String = req.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        } else {
            None
        }
    }).unwrap_or(0)
}

pub fn evaluate_goal_satisfied(spec: Option<&GoalSpec>, workspace: &Path) -> bool {
    let Some(spec) = spec else {
        return false;
    };
    let required_loc = extract_loc_requirement(spec);
    if required_loc == 0 {
        return false;
    }
    count_loc(workspace) >= required_loc
}
