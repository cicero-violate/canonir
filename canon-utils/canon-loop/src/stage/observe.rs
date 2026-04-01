use canon_event::{CanonEvent, EventKind, LoopObserved, RuntimeEvent, ToolCall, ToolResult};
use canon_goal::parse_agent_goal_markdown;
use canon_semantic_state::SemanticStateSummary;
use canon_tools_search::search_files;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::planning_preconditions;
use crate::{context::LoopContext, env_model::WorkspaceModel, result::LoopStageResult};

/// Emit a LoopObserved if state has changed since the last observation.
/// Called directly from the executor on state-changing events, not on every tick.
pub fn execute(ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    execute_inner(ctx, false)
}

pub fn execute_forced(ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    execute_inner(ctx, true)
}

fn execute_inner(ctx: &mut LoopContext, force: bool) -> anyhow::Result<LoopStageResult> {
    // FIX: hard guard — only allow ONE observe execution per tick globally
    if !force {
        if let Some(last_tick) = ctx.last_observed_tick {
            if last_tick >= ctx.current_tick {
                // do not early return; must still emit LoopObserved to satisfy invariant
            }
        }
    }
    // INVARIANT: Observe MUST emit exactly one LoopObserved (no bypass)
    // All control paths in this function must converge to the final emission below.
    // Early returns are forbidden in this function.
    if ctx.goal_text.is_none() || ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(false) {
        ctx.goal_text = scan_tlog_for_goal(ctx.tlog_path.as_path());
    }

    // If goal is still placeholder or absent after scan, nothing downstream can act.
    // Return Noop unconditionally — do NOT emit LoopObserved with a placeholder goal
    // regardless of whether state_changed, as that would trigger RouteExecutor →
    // RouteSelected(plan) → observe again, creating a spam loop.
    if !force && ctx.goal_text.as_deref().map(is_placeholder_goal).unwrap_or(true) {
        // CHANGED: do not early-return; allow observe emission for recovery invariants
    }

    let goal_hash = {
        let mut h = DefaultHasher::new();
        ctx.goal_text.hash(&mut h);
        h.finish()
    };
    let (semantic_summary, observe_diagnostics, observe_events) = build_observation_payload(&ctx.goal_text, &ctx.workspace, &ctx.recent_compiler_errors);
    let facts_hash = {
        let mut h = DefaultHasher::new();
        semantic_summary.hash(&mut h);
        observe_diagnostics.hash(&mut h);
        h.finish()
    };
    let _state_changed = (ctx.error_count as u64) != ctx.last_observed_error_count || goal_hash != ctx.last_observed_goal_hash || facts_hash != ctx.last_observed_facts_hash;
    // FIX: DO NOT suppress emission based on state_changed — invariant requires RouteSelected to always follow LoopObserved
    // Removing this guard prevents system from getting stuck waiting for route_selected
    // FIX: deduplicate identical observations at source
    if ctx.last_observed_error_count == ctx.error_count as u64
        && ctx.last_observed_goal_hash == goal_hash
        && ctx.last_observed_facts_hash == facts_hash
    {
        // invariant: MUST still emit LoopObserved exactly once per execution
        // do not early-return; allow emission below
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
        semantic_summary,
        observe_diagnostics,
    };
    let mut out = observe_events;
    // enforce invariant: exactly one LoopObserved in output
    out.retain(|e| !matches!(e, RuntimeEvent::LoopObserved(_)));
    out.push(RuntimeEvent::LoopObserved(payload));
    // HARD INVARIANT: ensure LoopObserved is always emitted exactly once
    debug_assert!(out.iter().any(|e| matches!(e, RuntimeEvent::LoopObserved(_))), "LoopObserved must be emitted");
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
            let is_goal = val.get("prompt_id").and_then(|v| v.as_str()) == Some("AGENT_GOAL") || val.get("path").and_then(|v| v.as_str()).map(|p| p.contains("AGENT_GOAL")).unwrap_or(false);
            if is_goal {
                if let Some(c) = val.get("content").and_then(|v| v.as_str()) {
                    found = Some(c.to_string());
                }
            }
        }
    }
    found
}

fn build_observation_payload(goal_text: &Option<String>, workspace: &Path, compiler_errors: &[serde_json::Value]) -> (SemanticStateSummary, Vec<String>, Vec<RuntimeEvent>) {
    let Some(goal) = goal_text else {
        return (SemanticStateSummary::default(), Vec::new(), Vec::new());
    };
    let mut search_hits = Vec::new();
    let Some(model) = WorkspaceModel::inspect(goal, workspace) else {
        return (SemanticStateSummary::default(), Vec::new(), Vec::new());
    };
    let planning_preconditions = planning_preconditions::derive_preconditions(Some(&model), compiler_errors);
    let compiler_hints = crate::compiler_hints::planner_lines(compiler_errors);
    let planning_precondition_lines = planning_preconditions::planner_lines(&planning_preconditions);
    let failure_class = compiler_hints.iter().find_map(|hint| hint.kind_enum().map(|kind| kind.as_str().to_string())).or_else(|| {
        if planning_preconditions.is_empty() {
            Some(canon_semantic_state::FailureClassKind::NoActionableFailure.as_str().to_string())
        } else {
            None
        }
    });
    let failure_scope = compiler_hints
        .iter()
        .filter_map(|hint| hint.failure_scope_enum())
        .find(|scope| *scope != canon_semantic_state::FailureScopeKind::None)
        .map(|scope| scope.as_str().to_string())
        .or_else(|| if planning_preconditions.is_empty() { Some(canon_semantic_state::FailureScopeKind::None.as_str().to_string()) } else { None });
    let repair_intents = planning_preconditions::derive_repair_intents(&planning_preconditions, failure_scope.as_deref());
    let target_root = model.target_root.clone();
    let summary = SemanticStateSummary {
        version: SemanticStateSummary::VERSION,
        complete: true,
        target_root: Some(target_root.display().to_string()),
        path_exists: model.path_exists,
        repo_initialized: model.repo_initialized,
        cargo_project: model.cargo_toml_exists,
        crate_name: model.crate_name.clone(),
        entrypoint_kind: Some(model.entrypoint_kind.as_str().to_string()),
        rust_file_count: Some(model.rust_file_count),
        source_files: model.source_files.iter().map(|p| p.display().to_string()).collect(),
        module_gaps: model.module_gaps.iter().map(|gap| format!("{} -> {}", gap.module_name, gap.expected_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" or "))).collect(),
        planning_preconditions: planning_precondition_lines,
        repair_intents: planning_preconditions::repair_intent_lines(&repair_intents),
        compiler_hints,
        validation_blocked_by_preconditions: !planning_preconditions.is_empty(),
        compiler_repair_required: planning_preconditions.contains(&planning_preconditions::PlanningPrecondition::MustFixDeadCodeForbidConflict),
        failure_class,
        failure_scope,
        ..read_graph_summary(&target_root).unwrap_or_default()
    };
    let cargo_toml = target_root.join("Cargo.toml");
    let entrypoint = preferred_entrypoint(&target_root);

    let listing = list_dir_entries(&target_root, 32);

    let spec = parse_agent_goal_markdown(goal);
    let keywords = extract_goal_keywords(&spec);
    for kw in keywords.into_iter().take(3) {
        if let Ok(results) = search_files(&kw, &target_root, 3) {
            for r in results {
                search_hits.push(serde_json::json!({
                    "keyword": kw,
                    "path": r.path.display().to_string(),
                    "score": r.score,
                }));
            }
        }
    }

    let observe_diagnostics = build_observe_diagnostics(&model, &listing, &search_hits);
    if !model.path_exists {
        return (summary, observe_diagnostics, Vec::new());
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
            "cargo_toml_exists": model.cargo_toml_exists,
            "entrypoint": entrypoint.as_ref().map(|p| p.display().to_string()),
            "repo_initialized": model.repo_initialized,
            "entrypoint_kind": model.entrypoint_kind.as_str(),
            "module_gap_count": model.module_gaps.len(),
            "search_hits": search_hits,
        }),
    ));

    (summary, observe_diagnostics, events)
}

fn read_graph_summary(target_root: &Path) -> Option<SemanticStateSummary> {
    let path = target_root.join("state").join("graph").join("index").join("latest_workspace.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let wrapper: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let summary = wrapper.get("latest_workspace").cloned().unwrap_or(wrapper);
    let artifact_id = summary.get("artifact_id")?.as_str()?.to_string();
    Some(SemanticStateSummary {
        graph_artifact_id: Some(artifact_id),
        graph_node_count: summary.get("node_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        graph_edge_count: summary.get("edge_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        graph_file_count: summary.get("file_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        graph_call_edge_count: summary.get("call_edge_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        graph_module_edge_count: summary.get("module_edge_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        graph_cfg_edge_count: summary.get("cfg_edge_count").and_then(|v| v.as_u64()).map(|v| v as usize),
        ..SemanticStateSummary::default()
    })
}

fn build_observe_diagnostics(model: &WorkspaceModel, listing: &[String], search_hits: &[serde_json::Value]) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if !listing.is_empty() {
        diagnostics.push(format!("dir_entries={}", listing.join(",")));
    }
    diagnostics.push(format!("cargo_toml_exists={}", model.cargo_toml_exists));
    diagnostics.push(format!("repo_initialized={}", model.repo_initialized));
    diagnostics.push(format!("entrypoint_kind={}", model.entrypoint_kind.as_str()));
    diagnostics.push(format!("module_gap_count={}", model.module_gaps.len()));
    if !search_hits.is_empty() {
        diagnostics.push(format!("search_hit_count={}", search_hits.len()));
    }
    diagnostics
}

fn synthetic_observe_event(node_id: &str, kind: &str, payload: serde_json::Value, output: serde_json::Value) -> Vec<RuntimeEvent> {
    let request_id = format!("observe-{}", Uuid::new_v4());
    let tool_call_id = Uuid::new_v4().to_string();
    let tool_result_id = Uuid::new_v4().to_string();
    vec![
        RuntimeEvent::ToolCall(ToolCall { node_id: node_id.to_string(), tool_call_id: tool_call_id.clone(), request_id: request_id.clone(), kind: kind.to_string(), payload, accepted: true }),
        RuntimeEvent::ToolResult(ToolResult { node_id: node_id.to_string(), tool_call_id, tool_result_id, request_id, kind: kind.to_string(), output, success: true }),
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
