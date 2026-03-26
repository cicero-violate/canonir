use canon_event::{CanonEvent, EventKind, LoopObserved, RuntimeEvent, ToolCall, ToolResult};
use canon_goal::parse_agent_goal_markdown;
use canon_tools_search::search_files;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::{context::LoopContext, result::LoopStageResult};

/// Emit a LoopObserved if state has changed since the last observation.
/// Called directly from the executor on state-changing events, not on every tick.
pub fn execute(ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    execute_inner(ctx, false)
}

pub fn execute_forced(ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    execute_inner(ctx, true)
}

fn execute_inner(ctx: &mut LoopContext, force: bool) -> anyhow::Result<LoopStageResult> {
    if ctx.goal_text.is_none() || ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(false) {
        ctx.goal_text = scan_tlog_for_goal(ctx.tlog_path.as_path());
    }

    // If goal is still placeholder or absent after scan, nothing downstream can act.
    // Return Noop unconditionally — do NOT emit LoopObserved with a placeholder goal
    // regardless of whether state_changed, as that would trigger RouteExecutor →
    // RouteSelected(plan) → observe again, creating a spam loop.
    if !force && ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(true) {
        return Ok(LoopStageResult::Noop);
    }

    let goal_hash = {
        let mut h = DefaultHasher::new();
        ctx.goal_text.hash(&mut h);
        h.finish()
    };
    let (workspace_facts, observe_events) = build_workspace_facts(&ctx.goal_text);
    let facts_hash = {
        let mut h = DefaultHasher::new();
        workspace_facts.hash(&mut h);
        h.finish()
    };
    let state_changed = (ctx.error_count as u64) != ctx.last_observed_error_count || goal_hash != ctx.last_observed_goal_hash || facts_hash != ctx.last_observed_facts_hash;
    if !force && !state_changed {
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
    let mut out = observe_events;
    out.push(RuntimeEvent::LoopObserved(payload));
    Ok(LoopStageResult::EmitMany(out))
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

fn build_workspace_facts(goal_text: &Option<String>) -> (Vec<String>, Vec<RuntimeEvent>) {
    let Some(goal) = goal_text else {
        return (Vec::new(), Vec::new());
    };
    let mut facts = Vec::new();
    let mut search_hits = Vec::new();
    let Some(target_root) = resolve_target_root(goal) else {
        return (facts, Vec::new());
    };

    let exists = target_root.exists();
    facts.push(format!("target_path_exists={} path={}", exists, target_root.display()));
    if !exists {
        return (facts, Vec::new());
    }

    let cargo_toml = target_root.join("Cargo.toml");
    let entrypoint = preferred_entrypoint(&target_root);
    facts.push(format!("cargo_toml_exists={} path={}", cargo_toml.exists(), cargo_toml.display()));
    if let Some(entry) = &entrypoint {
        facts.push(format!("entrypoint_exists=true path={}", entry.display()));
    } else {
        facts.push("entrypoint_exists=false path=NA".to_string());
    }

    let listing = list_dir_entries(&target_root, 32);
    if !listing.is_empty() {
        facts.push(format!("dir_entries={}", listing.join(",")));
    }

    let spec = parse_agent_goal_markdown(goal);
    let keywords = extract_goal_keywords(&spec);
    for kw in keywords.into_iter().take(3) {
        if let Ok(results) = search_files(&kw, &target_root, 3) {
            for r in results {
                facts.push(format!("search_match kw={} path={}", kw, r.path.display()));
                search_hits.push(serde_json::json!({
                    "keyword": kw,
                    "path": r.path.display().to_string(),
                    "score": r.score,
                }));
            }
        }
    }

    let node_id = "observe_consumer".to_string();
    let mut events = Vec::new();
    events.extend(synthetic_observe_event(
        &node_id,
        "observe.list_dir",
        serde_json::json!({
            "path": target_root.display().to_string(),
        }),
        serde_json::json!({
            "path": target_root.display().to_string(),
            "entries": listing,
        }),
    ));
    if cargo_toml.exists() {
        let cargo_contents = read_file_preview(&cargo_toml, 4000);
        events.extend(synthetic_observe_event(
            &node_id,
            "observe.read_file",
            serde_json::json!({
                "path": cargo_toml.display().to_string(),
            }),
            serde_json::json!({
                "path": cargo_toml.display().to_string(),
                "stdout": cargo_contents,
            }),
        ));
    }
    if let Some(entry) = &entrypoint {
        let entry_contents = read_file_preview(entry, 4000);
        events.extend(synthetic_observe_event(
            &node_id,
            "observe.read_file",
            serde_json::json!({
                "path": entry.display().to_string(),
            }),
            serde_json::json!({
                "path": entry.display().to_string(),
                "stdout": entry_contents,
            }),
        ));
    }
    events.extend(synthetic_observe_event(
        &node_id,
        "observe.search",
        serde_json::json!({
            "target_root": target_root.display().to_string(),
            "keywords": extract_goal_keywords(&spec).into_iter().take(3).collect::<Vec<_>>(),
        }),
        serde_json::json!({
            "op": "workspace_scan",
            "target_root": target_root.display().to_string(),
            "cargo_toml_exists": cargo_toml.exists(),
            "entrypoint": entrypoint.as_ref().map(|p| p.display().to_string()),
            "search_hits": search_hits,
        }),
    ));

    (facts, events)
}

fn synthetic_observe_event(
    node_id: &str,
    kind: &str,
    payload: serde_json::Value,
    output: serde_json::Value,
) -> Vec<RuntimeEvent> {
    let request_id = format!("observe-{}", Uuid::new_v4());
    let tool_call_id = Uuid::new_v4().to_string();
    let tool_result_id = Uuid::new_v4().to_string();
    vec![
        RuntimeEvent::ToolCall(ToolCall {
            node_id: node_id.to_string(),
            tool_call_id: tool_call_id.clone(),
            request_id: request_id.clone(),
            kind: kind.to_string(),
            payload,
            accepted: true,
        }),
        RuntimeEvent::ToolResult(ToolResult {
            node_id: node_id.to_string(),
            tool_call_id,
            tool_result_id,
            request_id,
            kind: kind.to_string(),
            output,
            success: true,
        }),
    ]
}

fn preferred_entrypoint(target_root: &Path) -> Option<PathBuf> {
    for candidate in ["src/main.rs", "src/lib.rs"] {
        let path = target_root.join(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn list_dir_entries(dir: &Path, limit: usize) -> Vec<String> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let suffix = if entry.path().is_dir() { "/" } else { "" };
            format!("{file_name}{suffix}")
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.truncate(limit);
    entries
}

fn read_file_preview(path: &Path, max_len: usize) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };
    if contents.len() > max_len {
        let mut truncated = contents[..max_len].to_string();
        truncated.push_str("...<truncated>");
        truncated
    } else {
        contents
    }
}

fn resolve_target_root(goal: &str) -> Option<PathBuf> {
    let raw = extract_target_path(goal)?;
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Some(path)
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
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

fn extract_goal_keywords(spec: &canon_goal::GoalSpec) -> Vec<String> {
    let mut out = Vec::new();
    for req in &spec.requirements {
        for token in req.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '/') {
            if token.len() >= 4 || token.contains('.') || token.contains('/') {
                out.push(token.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
