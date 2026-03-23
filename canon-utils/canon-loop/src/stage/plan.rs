use std::path::Path;

use canon_event::{CapabilityCompleted, CapabilityFailed, CapabilityResult, RuntimeEvent, LlmCall, LoopActed, LoopObserved, LoopPlanned, RouteSelected, ToolCall, ToolResult};
use canon_goal::parse_agent_goal_markdown;
use canon_tools_search::search_files;
use uuid::Uuid;

use crate::{context::{LoopContext, PendingPlan}, result::LoopStageResult};

const LLM_TIMEOUT_TICKS: u64 = 60;

pub fn execute_trigger(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let tick = rs.tick;
    check_llm_timeout(ctx, tick);
    let Some(observed) = ctx.last_observed.clone() else {
        return Ok(LoopStageResult::Noop);
    };
    handle_observed(ctx, &observed)
}

pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != c.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }

    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, true)?;

    let (mut actions, signals) = match &c.result {
        CapabilityResult::Llm(llm) => parse_llm_actions(&llm.response),
        _ => (Vec::new(), None::<serde_json::Value>),
    };
    ctx.last_llm_signals = signals.clone();
    if actions.len() > 1 && actions.iter().any(|a| matches!(a.action, LlmAction::Done { .. })) {
        actions.retain(|a| !matches!(a.action, LlmAction::Done { .. }));
    }
    if actions.is_empty() {
        return emit_plan(ctx, LoopPlanned {
            tick: pending.tick,
            action_kind: "no_op".to_string(),
            action_payload: serde_json::json!({}),
            reason: "llm_parse_failed".to_string(),
            llm_request_id: Some(pending.request_id.clone()),
            signals: signals.clone(),
            trace_id: Some(pending.trace_id.clone()),
            execution_id: Some(pending.execution_id.clone()),
            span_id: None,
            parent_span_id: Some(pending.span_id.clone()),
            plan_id: Some(pending.plan_id.clone()),
            plan_step_id: None,
            action_id: None,
            depends_on: Vec::new(),
        });
    }

    let req_id = pending.request_id.clone();
    let mut out = Vec::new();
    for action in actions {
        let plan_step_id = Uuid::new_v4().to_string();
        let action_id = plan_step_id.clone();
        let planned_span_id = Uuid::new_v4().to_string();
        match action.action {
            LlmAction::Patch { path, old, new } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "patch_file".to_string(),
                action_payload: serde_json::json!({ "path": path, "old": old, "new": new }),
                reason: "llm_patch".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ApplyPatch { patch } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "apply_patch".to_string(),
                action_payload: serde_json::json!({ "patch": patch }),
                reason: "llm_apply_patch".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::Command { cmd, cwd } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "run_command".to_string(),
                action_payload: action_payload_with_cwd(cmd, cwd),
                reason: "llm_command".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::Write { path, content } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "write_file".to_string(),
                action_payload: serde_json::json!({ "path": path, "content": content }),
                reason: "llm_write".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ReadFile { path } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "read_file".to_string(),
                action_payload: serde_json::json!({ "path": path }),
                reason: "llm_read_file".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::ListDir { path } => out.push(LoopPlanned {
                tick: pending.tick,
                action_kind: "list_dir".to_string(),
                action_payload: serde_json::json!({ "path": path }),
                reason: "llm_list_dir".to_string(),
                llm_request_id: Some(req_id.clone()),
                trace_id: Some(pending.trace_id.clone()),
                execution_id: Some(pending.execution_id.clone()),
                span_id: Some(planned_span_id.clone()),
                parent_span_id: Some(pending.span_id.clone()),
                plan_id: Some(pending.plan_id.clone()),
                plan_step_id: Some(plan_step_id.clone()),
                action_id: Some(action_id.clone()),
                signals: signals.clone(),
                depends_on: action.depends_on.clone(),
            }),
            LlmAction::Done { reason } => {
                if let Some(goal_text) = &pending.goal_text {
                    let required_loc = extract_required_loc(goal_text);
                    let satisfied = required_loc == 0 || count_loc_in_workspace(&ctx.workspace) >= required_loc;
                    if satisfied {
                        ctx.last_done_goal = pending.goal_text.clone();
                    }
                } else {
                    ctx.last_done_goal = pending.goal_text.clone();
                }
                out.push(LoopPlanned {
                    tick: pending.tick,
                    action_kind: "done".to_string(),
                    action_payload: serde_json::json!({}),
                    reason,
                    llm_request_id: Some(req_id.clone()),
                    trace_id: Some(pending.trace_id.clone()),
                    execution_id: Some(pending.execution_id.clone()),
                    span_id: Some(planned_span_id.clone()),
                    parent_span_id: Some(pending.span_id.clone()),
                    plan_id: Some(pending.plan_id.clone()),
                    plan_step_id: Some(plan_step_id.clone()),
                    action_id: Some(action_id.clone()),
                    signals: signals.clone(),
                    depends_on: action.depends_on.clone(),
                });
            }
        }
    }
    if out.is_empty() {
        return Ok(LoopStageResult::Noop);
    }
    Ok(LoopStageResult::EmitMany(out.into_iter().map(RuntimeEvent::LoopPlanned).collect()))
}

pub fn execute_failed(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending) = ctx.pending_plan.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending.request_id != f.request_id {
        ctx.pending_plan = Some(pending);
        return Ok(LoopStageResult::Noop);
    }
    emit_tool_result(ctx, &pending.plan_tool_call_id, &pending.request_id, false)?;
    emit_plan(ctx, LoopPlanned {
        tick: pending.tick,
        action_kind: "no_op".to_string(),
        action_payload: serde_json::json!({}),
        reason: "llm_failed".to_string(),
        llm_request_id: Some(pending.request_id),
        trace_id: Some(pending.trace_id),
        execution_id: Some(pending.execution_id),
        span_id: None,
        parent_span_id: Some(pending.span_id),
        plan_id: Some(pending.plan_id),
        plan_step_id: None,
        action_id: None,
        signals: None,
        depends_on: Vec::new(),
    })
}

fn handle_observed(ctx: &mut LoopContext, observed: &LoopObserved) -> anyhow::Result<LoopStageResult> {
    if ctx.pending_plan.is_some() || ctx.last_planned_observed_tick == Some(observed.tick) {
        return Ok(LoopStageResult::Noop);
    }
    if observed.goal_text.is_none() && observed.error_count == 0 {
        return emit_plan(ctx, LoopPlanned {
            tick: observed.tick,
            action_kind: "no_op".to_string(),
            action_payload: serde_json::json!({}),
            reason: "no_goal".to_string(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            signals: None,
            depends_on: Vec::new(),
        });
    }
    if observed.error_count == 0 && ctx.last_done_goal.is_some() && ctx.last_done_goal == observed.goal_text {
        if requirements_satisfied(ctx, observed) {
            return emit_plan(ctx, LoopPlanned {
                tick: observed.tick,
                action_kind: "no_op".to_string(),
                action_payload: serde_json::json!({}),
                reason: "goal_complete".to_string(),
                llm_request_id: None,
                signals: None,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
                depends_on: Vec::new(),
            });
        }
        ctx.last_done_goal = None;
    }

    let request_id = Uuid::new_v4().to_string();
    let trace_id = Uuid::new_v4().to_string();
    let execution_id = Uuid::new_v4().to_string();
    let span_id = Uuid::new_v4().to_string();
    let plan_id = Uuid::new_v4().to_string();
    let include_full_goal = observed.goal_text != ctx.last_prompted_goal;
    let prompt = build_prompt(
        observed,
        &ctx.batch_acted,
        &ctx.batch_tool_results,
        include_full_goal,
        &ctx.workspace,
        &ctx.context_merger.prompt_section(),
    );
    let llm_call = LlmCall { request_id: request_id.clone(), prompt: prompt.to_string(), role: Some("planner".to_string()), agent_id: None };

    let plan_tool_call_id = Uuid::new_v4().to_string();
    ctx.batch_acted.clear();
    ctx.batch_tool_results.clear();
    ctx.pending_plan = Some(PendingPlan {
        tick: observed.tick,
        request_id: request_id.clone(),
        dispatched_at_tick: observed.tick,
        goal_text: observed.goal_text.clone(),
        trace_id,
        execution_id,
        span_id,
        plan_id,
        plan_tool_call_id: plan_tool_call_id.clone(),
    });
    if include_full_goal {
        ctx.last_prompted_goal = observed.goal_text.clone();
    }
    ctx.last_planned_observed_tick = Some(observed.tick);

    if let Some(emitter) = ctx.emitter.as_ref() {
        canon_meta::canon_emit_meta!(emitter; ToolCall(ToolCall {
            node_id: "plan_consumer".to_string(),
            tool_call_id: plan_tool_call_id,
            request_id: request_id.clone(),
            kind: "llm.plan".to_string(),
            payload: serde_json::json!({"role": "planner"}),
        }));
        canon_meta::canon_emit_meta!(emitter; Llm(LlmCall {
            request_id,
            prompt: llm_call.prompt,
            role: llm_call.role,
            agent_id: None,
        }));
    }

    Ok(LoopStageResult::Deferred)
}

fn requirements_satisfied(ctx: &LoopContext, observed: &LoopObserved) -> bool {
    let Some(goal_text) = observed.goal_text.as_ref() else {
        return false;
    };
    let required_loc = extract_required_loc(goal_text);
    if required_loc == 0 {
        return true;
    }
    let actual_loc = count_loc_in_workspace(&ctx.workspace);
    actual_loc >= required_loc
}

fn check_llm_timeout(ctx: &mut LoopContext, current_tick: u64) {
    let Some(pending) = &ctx.pending_plan else {
        return;
    };
    if current_tick.saturating_sub(pending.dispatched_at_tick) < LLM_TIMEOUT_TICKS {
        return;
    }
    let tick = pending.tick;
    ctx.pending_plan = None;
    let _ = emit_plan(ctx, LoopPlanned {
        tick,
        action_kind: "no_op".to_string(),
        action_payload: serde_json::json!({}),
        reason: "llm_timeout".to_string(),
        llm_request_id: None,
        signals: None,
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
        plan_id: None,
        plan_step_id: None,
        action_id: None,
        depends_on: Vec::new(),
    });
}

fn emit_plan(_ctx: &LoopContext, payload: LoopPlanned) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopPlanned(payload)))
}

fn emit_tool_result(ctx: &LoopContext, tool_call_id: &str, request_id: &str, success: bool) -> anyhow::Result<()> {
    if let Some(emitter) = ctx.emitter.as_ref() {
        canon_meta::canon_emit_meta!(emitter; ToolResult(ToolResult {
            node_id: "plan_consumer".to_string(),
            tool_call_id: tool_call_id.to_string(),
            tool_result_id: Uuid::new_v4().to_string(),
            request_id: request_id.to_string(),
            kind: "llm.plan".to_string(),
            output: serde_json::json!({}),
            success,
        }));
    }
    Ok(())
}

#[derive(Clone)]
enum LlmAction {
    Patch { path: String, old: String, new: String },
    Write { path: String, content: String },
    Command { cmd: String, cwd: Option<String> },
    Done { reason: String },
    ApplyPatch { patch: String },
    ReadFile { path: String },
    ListDir { path: String },
}

#[derive(Clone)]
struct ActionPlan {
    action: LlmAction,
    depends_on: Vec<String>,
}


fn build_prompt(
    observed: &LoopObserved,
    batch_acted: &[LoopActed],
    batch_tool_results: &[ToolResult],
    _include_full_goal: bool,
    workspace: &Path,
    sub_agent_section: &str,
) -> String {
    let recent_actions = batch_acted
        .iter()
        .rev()
        .take(8)
        .map(|a| {
            let mut entry = format!(
                "- action={} success={} exit_code={:?}",
                a.action_kind, a.success, a.exit_code,
            );
            // Include actual content so the LLM can learn from failures and read results.
            let stdout = a.stdout.trim();
            let stderr = a.stderr.trim();
            if !stdout.is_empty() {
                let truncated = if stdout.len() > 800 { &stdout[..800] } else { stdout };
                entry.push_str(&format!("\n  stdout: {truncated}"));
            }
            if !stderr.is_empty() {
                let truncated = if stderr.len() > 400 { &stderr[..400] } else { stderr };
                entry.push_str(&format!("\n  stderr: {truncated}"));
            }
            entry
        })
        .collect::<Vec<_>>()
        .join("\n");

    let recent_results = batch_tool_results
        .iter()
        .rev()
        .take(4)
        .map(|r| {
            let content = serde_json::to_string_pretty(&r.output).unwrap_or_else(|_| r.output.to_string());
            let truncated = if content.len() > 600 { &content[..600] } else { &content };
            format!("- kind={} success={}\n  output: {}", r.kind, r.success, truncated)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let goal_text = observed.goal_text.clone().unwrap_or_else(|| "<no goal provided>".to_string());
    let workspace_loc = count_loc_in_workspace(workspace);

    let spec = parse_agent_goal_markdown(&goal_text);
    let target_workspace = spec.target_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| workspace.display().to_string());

    let search_hints = build_search_hints(&goal_text, workspace);
    let workspace_tree = build_workspace_tree(std::path::Path::new(&target_workspace), 3, 0);
    let workspace_facts = if observed.workspace_facts.is_empty() {
        " (none)".to_string()
    } else {
        observed.workspace_facts.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")
    };
    let destructive_warning = batch_acted.iter().any(|a| a.stderr.trim() == "rejected_destructive_command");
    let destructive_note = if destructive_warning {
        "\nWARNING: A previous plan was blocked as destructive. Do NOT include destructive commands; they will fail.\n"
    } else {
        "\n"
    };

    format!(
        r#"You are a code-editing agent. Produce a plan as a JSON array of actions.

TARGET WORKSPACE: {target_workspace}
All relative paths resolve against TARGET WORKSPACE.
LOC: {loc}  |  Errors: {errors}  |  Warnings: {warnings}
{destructive_note}
GOAL:
{goal}

## Workspace State
{workspace_tree}

Workspace facts:
{workspace_facts}

━━━ TOOLS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. list_dir — list what files/dirs exist (use BEFORE assuming project state)
   {{"action":"list_dir","path":"."}}

2. read_file — read a file's current contents (use BEFORE editing it)
   {{"action":"read_file","path":"src/main.rs"}}
   ⚠ Results appear in "Recent actions" on your NEXT call. Do not mix with edits.

3. apply_patch — create, update, or delete files  ← ONLY tool for file edits
   {{"action":"apply_patch","patch":"*** Begin Patch\n...\n*** End Patch"}}

   Patch format (paths MUST be relative to TARGET WORKSPACE):

   *** Begin Patch
   *** Add File: path/to/new.rs        ← create new file
   +fn hello() {{}}
   +
   *** Update File: path/to/existing.rs ← edit existing file
   @@ fn main                           ← optional context (function/class name)
    fn main() {{                        ←  space = unchanged context line
   -    println!("old");               ← - = remove this line
   +    println!("new");               ← + = add this line
    }}
   *** Delete File: path/to/remove.rs  ← delete file
   *** End Patch

   Rules:
   - *** Add File for new files, *** Update File for existing files
   - Include 3 lines of unchanged context around each change
   - Multiple file ops can be in one patch
   - NEVER use absolute paths inside the patch string

4. run_command — run a shell command
   {{"action":"run_command","cmd":"cargo build","cwd":"{target_workspace}"}}
   cwd must be absolute. Use TARGET WORKSPACE or a subdir.

5. done — declare goal complete
   {{"action":"done","reason":"..."}}

━━━ WORKFLOW ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Step 1 — Discover (when unsure of project state):
  Emit ONLY list_dir and/or read_file. Do NOT mix with edits.
  → Results appear in "Recent actions" on your next call.

Step 2 — Create/Edit (after seeing discovery results):
  Use apply_patch (*** Add File for new, *** Update File for existing).
  Use run_command for cargo/shell operations.
  The "done" action must be the ONLY action in a batch, and only after verification has shown the goal is met.

NEVER use "write" or "patch_file" — they are removed. Use apply_patch.
NEVER assume a directory/project exists without checking with list_dir first.
WORKSPACE RULE: If the target project directory already exists in the workspace tree, use `cargo init --name <name>` instead of `cargo new`. `cargo new` fails when the directory exists.
SAFETY RULE: The following commands are BLOCKED and will always fail. Do NOT plan them: rm -rf, git reset --hard, git clean -f, dd if=, mkfs, shred, >/dev/sd.

━━━ CONTEXT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Relevant files:{search_hints}

Recent actions (most recent first — read_file stdout contains file contents):
{recent_actions}

Recent tool results:
{recent_results}

{sub_agent_section}

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Return ONLY a JSON array of action objects. Do NOT wrap in an object. Do NOT include a "signals" key.
Example:
[
  {{"action":"list_dir","path":"."}},
  {{"action":"run_command","cmd":"cargo build","cwd":"{target_workspace}"}}
]

Rules:
- If you believe the goal is complete, the array must contain exactly one item: {{"action":"done","reason":"..."}}
- Never include "done" alongside any other action.
No prose outside the code block.
"#,
        target_workspace = target_workspace,
        goal = goal_text,
        workspace_tree = workspace_tree,
        loc = workspace_loc,
        errors = observed.error_count,
        warnings = observed.warning_count,
        destructive_note = destructive_note,
        search_hints = search_hints,
        recent_actions = if recent_actions.is_empty() { "(none)".to_string() } else { recent_actions }.to_string(),
        recent_results = if recent_results.is_empty() { "(none)".to_string() } else { recent_results }.to_string(),
        workspace_facts = workspace_facts,
        sub_agent_section = sub_agent_section,
    )
}

fn build_search_hints(goal_text: &str, workspace: &Path) -> String {
    let spec = parse_agent_goal_markdown(goal_text);
    let target_root = spec.target_path.clone().map(|p| workspace.join(p)).unwrap_or_else(|| workspace.to_path_buf());
    if !target_root.exists() {
        return " (none)".to_string();
    }

    let keywords = extract_goal_keywords(&spec);
    if keywords.is_empty() {
        return " (none)".to_string();
    }

    let mut lines = Vec::new();
    for kw in keywords.into_iter().take(3) {
        if let Ok(results) = search_files(&kw, &target_root, 5) {
            for r in results {
                lines.push(format!("\n- {kw}: {}", r.path.display()));
            }
        }
    }
    if lines.is_empty() {
        " (none)".to_string()
    } else {
        lines.join("")
    }
}

fn extract_goal_keywords(spec: &canon_goal::GoalSpec) -> Vec<String> {
    let mut out = Vec::new();
    for req in &spec.requirements {
        for token in req.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '/' ) {
            if token.len() >= 4 || token.contains('.') || token.contains('/') {
                out.push(token.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Parse LLM response into actions and an optional signals object.
/// Supports three response shapes:
///   A) {"signals":{...},"actions":[...]}  — wrapper format (preferred)
///   B) bare JSON array                    — legacy / fence-stripped
///   C) {"text":"```json\n...\n```"}       — text wrapper with fenced blocks
fn parse_llm_actions(result: &serde_json::Value) -> (Vec<ActionPlan>, Option<serde_json::Value>) {
    // Shape A: wrapper object with "actions" key
    if result.is_object() && result.get("actions").is_some() {
        let signals = result.get("signals").cloned();
        let actions = result["actions"].as_array()
            .map(|arr| arr.iter().filter_map(|v| parse_value_to_action(v.clone())).collect())
            .unwrap_or_default();
        return (actions, signals);
    }

    // Shape B: bare JSON array
    if let Some(arr) = result.as_array() {
        let actions = arr.iter().filter_map(|v| parse_value_to_action(v.clone())).collect();
        return (actions, None);
    }

    // Shape B: single action object
    if result.is_object() && result.get("action").is_some() {
        if let Some(action) = parse_value_to_action(result.clone()) {
            return (vec![action], None);
        }
    }

    // Shape C: {"text":"```json\n...\n```"} — extract fenced blocks
    let Some(text) = result.get("text").and_then(|v| v.as_str()) else {
        return (Vec::new(), None);
    };
    let blocks = extract_fenced_blocks(text);
    let mut actions = Vec::new();
    let mut signals: Option<serde_json::Value> = None;
    for block in blocks {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&block) {
            // Check for wrapper shape inside fenced block
            if parsed.is_object() && parsed.get("actions").is_some() {
                signals = parsed.get("signals").cloned();
                if let Some(arr) = parsed["actions"].as_array() {
                    for v in arr {
                        if let Some(a) = parse_value_to_action(v.clone()) { actions.push(a); }
                    }
                }
                continue;
            }
            if let Some(arr) = parsed.as_array() {
                for v in arr {
                    if let Some(a) = parse_value_to_action(v.clone()) { actions.push(a); }
                }
            } else if let Some(a) = parse_value_to_action(parsed) {
                actions.push(a);
            }
        }
    }
    (actions, signals)
}

fn extract_required_loc(goal_text: &str) -> usize {
    goal_text
        .lines()
        .filter(|l| l.to_lowercase().contains("loc"))
        .find_map(|l| {
            let digits: String = l.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().ok()
        })
        .unwrap_or(0)
}

fn count_loc_in_workspace(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += count_loc_in_workspace(&path);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                total += content.lines().count();
            }
        }
    }
    total
}

/// Build a compact directory tree (depth-limited, capped at 40 lines).
/// Skips hidden dirs (`.git`, `target`, `node_modules`).
fn build_workspace_tree(dir: &Path, max_depth: usize, depth: usize) -> String {
    const MAX_LINES: usize = 40;
    let mut lines: Vec<String> = Vec::new();
    build_workspace_tree_inner(dir, depth, max_depth, &mut lines, MAX_LINES);
    if lines.is_empty() {
        "(directory not found or empty)".to_string()
    } else {
        lines.join("\n")
    }
}

fn build_workspace_tree_inner(dir: &Path, depth: usize, max_depth: usize, lines: &mut Vec<String>, cap: usize) {
    if lines.len() >= cap { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    let indent = "  ".repeat(depth);
    let mut items: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .collect();
    items.sort();
    for path in items {
        if lines.len() >= cap { break; }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip hidden and build dirs
        if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
        if path.is_dir() {
            lines.push(format!("{indent}{name}/"));
            if depth < max_depth {
                build_workspace_tree_inner(&path, depth + 1, max_depth, lines, cap);
            }
        } else {
            lines.push(format!("{indent}{name}"));
        }
    }
}

fn extract_fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") {
                in_fence = true;
                current.clear();
            }
            continue;
        }

        if trimmed.starts_with("```") {
            blocks.push(current.trim().to_string());
            in_fence = false;
            current.clear();
            continue;
        }

        current.push_str(line);
        current.push('\n');
    }

    blocks
}

fn parse_value_to_action(value: serde_json::Value) -> Option<LlmAction> {
    // Handle "action" discriminator format (used by planner GPT)
    if let Some(action_str) = value.get("action").and_then(|v| v.as_str()) {
        match action_str {
            "done" => {
                let reason = value.get("reason").and_then(|v| v.as_str()).unwrap_or("done").to_string();
                return Some(LlmAction::Done { reason });
            }
            "apply_patch" => {
                let patch = value.get("patch").and_then(|v| v.as_str())?;
                return Some(LlmAction::ApplyPatch { patch: patch.to_string() });
            }
            "write" | "write_file" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let content = value.get("content").and_then(|v| v.as_str())?;
                return Some(LlmAction::Write { path: path.to_string(), content: content.to_string() });
            }
            "run_command" | "command" => {
                let cmd = value.get("cmd").and_then(|v| v.as_str())?;
                let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                return Some(LlmAction::Command { cmd: cmd.to_string(), cwd });
            }
            "patch_file" | "patch" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                let old = value.get("old").and_then(|v| v.as_str())?;
                let new = value.get("new").and_then(|v| v.as_str())?;
                return Some(LlmAction::Patch { path: path.to_string(), old: old.to_string(), new: new.to_string() });
            }
            "read_file" => {
                let path = value.get("path").and_then(|v| v.as_str())?;
                return Some(LlmAction::ReadFile { path: path.to_string() });
            }
            "list_dir" => {
                let path = value.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                return Some(LlmAction::ListDir { path: path.to_string() });
            }
            _ => return None,
        }
    }
    // Fallback: key-based schema (no "action" field)
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        let reason = value.get("reason").and_then(|v| v.as_str()).unwrap_or("done").to_string();
        return Some(LlmAction::Done { reason });
    }
    if let Some(cmd) = value.get("cmd").and_then(|v| v.as_str()) {
        let cwd = value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
        return Some(LlmAction::Command { cmd: cmd.to_string(), cwd });
    }
    if let Some(patch) = value.get("patch").and_then(|v| v.as_str()) {
        return Some(LlmAction::ApplyPatch { patch: patch.to_string() });
    }
    if let (Some(write_path), Some(content)) = (value.get("write").and_then(|v| v.as_str()), value.get("content").and_then(|v| v.as_str())) {
        return Some(LlmAction::Write { path: write_path.to_string(), content: content.to_string() });
    }
    let path = value.get("path").and_then(|v| v.as_str())?;
    let old = value.get("old").and_then(|v| v.as_str())?;
    let new = value.get("new").and_then(|v| v.as_str())?;
    Some(LlmAction::Patch { path: path.to_string(), old: old.to_string(), new: new.to_string() })
}

fn action_payload_with_cwd(cmd: String, cwd: Option<String>) -> serde_json::Value {
    let cwd = cwd.unwrap_or_else(|| ".".to_string());
    serde_json::json!({ "cmd": cmd, "cwd": cwd })
}
