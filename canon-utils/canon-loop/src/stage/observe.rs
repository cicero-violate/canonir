use canon_event::{CanonEvent, EventKind, LoopObserved, RuntimeEvent};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::{context::LoopContext, result::LoopStageResult};

/// Emit a LoopObserved if state has changed since the last observation.
/// Called directly from the executor on state-changing events, not on every tick.
pub fn execute(ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    if ctx.goal_text.is_none() || ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(false) {
        ctx.goal_text = scan_tlog_for_goal(ctx.tlog_path.as_path());
    }

    // If goal is still placeholder or absent after scan, nothing downstream can act.
    // Return Noop unconditionally — do NOT emit LoopObserved with a placeholder goal
    // regardless of whether state_changed, as that would trigger RouteExecutor →
    // RouteSelected(plan) → observe again, creating a spam loop.
    if ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(true) {
        return Ok(LoopStageResult::Noop);
    }

    let goal_hash = {
        let mut h = DefaultHasher::new();
        ctx.goal_text.hash(&mut h);
        h.finish()
    };
    let workspace_facts = build_workspace_facts(&ctx.goal_text);
    let facts_hash = {
        let mut h = DefaultHasher::new();
        workspace_facts.hash(&mut h);
        h.finish()
    };
    let state_changed = (ctx.error_count as u64) != ctx.last_observed_error_count || goal_hash != ctx.last_observed_goal_hash || facts_hash != ctx.last_observed_facts_hash;
    if !state_changed {
        let goal_pending = ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(true);
        // Wait state: goal is pending and no errors — nothing downstream can act.
        // Suppress even the stale heartbeat; only wake on genuine state change.
        if goal_pending {
            return Ok(LoopStageResult::Noop);
        }
        // Active state but nothing changed: only emit the stale heartbeat every 5 ticks.
        let stale = ctx.last_observed_tick.map(|t| ctx.current_tick.saturating_sub(t) >= 5).unwrap_or(true);
        if !stale {
            return Ok(LoopStageResult::Noop);
        }
    }
    ctx.last_observed_error_count = ctx.error_count as u64;
    ctx.last_observed_goal_hash = goal_hash;
    ctx.last_observed_facts_hash = facts_hash;
    // Update last_observed_tick inline so that subsequent trigger_observe calls within the
    // same event-processing window (before this LoopObserved loops back to the consumer)
    // correctly see stale=false and return Noop, not another observation.
    ctx.last_observed_tick = Some(ctx.current_tick);
    let payload = LoopObserved {
        tick: ctx.current_tick,
        error_count: ctx.error_count,
        warning_count: ctx.warning_count,
        compiler_errors: ctx.recent_compiler_errors.clone(),
        goal_text: ctx.goal_text.clone(),
        workspace_facts,
    };
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopObserved(payload)))
}

fn is_placeholder_goal(goal: &str) -> bool {
    let trimmed = goal.trim();
    trimmed.is_empty() || trimmed.contains("goal-pending")
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
            if ev.kind != EventKind::PromptLoaded {
                continue;
            }
            let val = &ev.payload.data;
            let is_goal = val.get("prompt_id").and_then(|v| v.as_str()) == Some("AGENT_GOAL")
                || val.get("path").and_then(|v| v.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false);
            if is_goal {
                if let Some(c) = val.get("content").and_then(|v| v.as_str()) {
                    found = Some(c.to_string());
                }
            }
        }
    }
    found
}

fn build_workspace_facts(goal_text: &Option<String>) -> Vec<String> {
    let Some(goal) = goal_text else {
        return Vec::new();
    };
    let target_path = extract_target_path(goal);
    match target_path {
        Some(p) => {
            let exists = Path::new(&p).exists();
            vec![format!("target_path_exists={} path={}", exists, p)]
        }
        None => Vec::new(),
    }
}

fn extract_target_path(goal: &str) -> Option<String> {
    goal.lines()
        .find_map(|line| {
            let trimmed = line.trim();
            // "Target: /path" or "target: /path"
            for prefix in &["Target:", "target:"] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let v = rest.trim().to_string();
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
            // "- Project path: /path" (markdown list under ## Target)
            for prefix in &["- Project path:", "- project path:", "Project path:"] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let v = rest.trim().trim_matches('`').to_string();
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
            None
        })
        .filter(|s| !s.is_empty())
}
