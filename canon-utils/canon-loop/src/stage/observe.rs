use canon_event::{CanonEvent, CanonPayload, RuntimeEvent, LoopObserved, Tick};
use std::path::{Path, PathBuf};

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute(t: Tick, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    if ctx.goal_text.is_none() {
        ctx.goal_text = scan_tlog_for_goal(ctx.tlog_path.as_path());
    }
    let payload = LoopObserved {
        tick: t.tick,
        error_count: ctx.error_count,
        warning_count: ctx.warning_count,
        compiler_errors: ctx.recent_compiler_errors.clone(),
        goal_text: ctx.goal_text.clone(),
    };
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopObserved(payload)))
}

/// Scan tlog segments (oldest first) for latest prompt_loaded of AGENT_GOAL.
fn scan_tlog_for_goal(tlog_path: &Path) -> Option<String> {
    let dir = if tlog_path.is_dir() { tlog_path.to_path_buf() } else { tlog_path.with_extension("tlog.d") };
    let mut logs: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().and_then(|s| s.to_str()) == Some("log")).collect();
    logs.sort();

    let mut found: Option<String> = None;
    for log_path in &logs {
        let content = match std::fs::read_to_string(log_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let Ok(ev) = serde_json::from_str::<CanonEvent>(line) else {
                continue;
            };
            if let CanonPayload::PromptLoaded(val) = ev.payload {
                let data = val.get("data").unwrap_or(&val);
                let is_goal = data.get("prompt_id").and_then(|v| v.as_str()) == Some("AGENT_GOAL")
                    || data.get("path").and_then(|v| v.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false);
                if is_goal {
                    if let Some(c) = data.get("content").and_then(|v| v.as_str()) {
                        found = Some(c.to_string());
                    }
                }
            }
        }
    }
    found
}
