use canon_event::{CapabilityCompleted, CapabilityResult, RuntimeEvent, RouteSelected, RequestDispatch, LlmCall, GoalNodeCreated, GoalEdgeDefined};
use crate::{context::LoopContext, result::LoopStageResult};
use uuid::Uuid;

/// Called when RouteSelected { route: "decompose" } arrives.
/// Emits an LlmCall asking the LLM to split the current goal into parallel sub-tasks.
/// Returns Deferred — the RequestDispatch events are emitted in execute_complete().
pub fn execute(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    // Already waiting on a decompose LLM response.
    if ctx.pending_decompose_request_id.is_some() {
        return Ok(LoopStageResult::Noop);
    }
    let goal = ctx.goal_text.clone()
        .or_else(|| ctx.last_observed.as_ref().and_then(|o| o.goal_text.clone()));
    let Some(goal_text) = goal else {
        return Ok(LoopStageResult::Noop);
    };
    let Some(emitter) = ctx.emitter.as_ref() else {
        return Ok(LoopStageResult::Noop);
    };

    let request_id = Uuid::new_v4().to_string();
    let prompt = build_decompose_prompt(&goal_text);
    ctx.pending_decompose_request_id = Some(request_id.clone());

    canon_meta::canon_emit_meta!(emitter; Llm(LlmCall {
        request_id,
        prompt,
        role: Some("decompose".to_string()),
        agent_id: ctx.agent_id.clone(),
    }));

    let _ = rs; // tick used for tracing only
    Ok(LoopStageResult::Deferred)
}

/// Called by dispatch_capability_done in mod.rs when CapabilityCompleted arrives.
/// Checks if this is our pending decompose response and, if so, parses it into RequestDispatches.
pub fn execute_complete(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let Some(pending_id) = ctx.pending_decompose_request_id.take() else {
        return Ok(LoopStageResult::Noop);
    };
    if pending_id != c.request_id {
        // Not ours — restore and pass through.
        ctx.pending_decompose_request_id = Some(pending_id);
        return Ok(LoopStageResult::Noop);
    }

    let response_text = match &c.result {
        CapabilityResult::Llm(res) => res.response.to_string(),
        _ => String::new(),
    };

    let parent_request_id = format!("decompose-{}", Uuid::new_v4());
    let dispatches = parse_decompose_tasks(&response_text, &parent_request_id);

    if dispatches.is_empty() {
        // LLM returned nothing useful — fall back to a single pass-through dispatch.
        let fallback = fallback_dispatch(&ctx.goal_text.clone().unwrap_or_default(), &parent_request_id);
        return Ok(LoopStageResult::EmitMany(vec![RuntimeEvent::RequestDispatch(fallback)]));
    }

    let mut events: Vec<RuntimeEvent> = Vec::new();
    for dispatch in &dispatches {
        events.push(RuntimeEvent::GoalNodeCreated(GoalNodeCreated {
            node_id:     dispatch.dispatch_id.clone(),
            description: dispatch.task_prompt.clone(),
            deps:        dispatch.deps.clone(),
            caps:        vec![dispatch.task_kind.clone()],
            node_type:   "sub_task".to_string(),
            priority:    128,
            budget:      None,
        }));
        for dep_id in &dispatch.deps {
            events.push(RuntimeEvent::GoalEdgeDefined(GoalEdgeDefined {
                from_node_id: dep_id.clone(),
                to_node_id:   dispatch.dispatch_id.clone(),
            }));
        }
        events.push(RuntimeEvent::RequestDispatch(dispatch.clone()));
    }
    Ok(LoopStageResult::EmitMany(events))
}

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

fn build_decompose_prompt(goal: &str) -> String {
    format!(
        r#"You are a task decomposition agent. Split the following goal into parallel sub-tasks that can be executed by specialist agents.

## Goal
{goal}

## Available agent roles
- "exec"       — writes code, applies patches, runs commands
- "doc_writer" — writes documentation, comments, README files
- "verifier"   — runs tests and verifies correctness

## Instructions
Respond with ONLY a JSON array. Each element must have:
  - "agent_id": one of the roles above
  - "task_kind": a short label ("implement", "document", "verify", etc.)
  - "task_prompt": the full instruction for that agent
  - "deps": (optional) array of 0-based indices of tasks this task depends on

Example:
[
  {{"agent_id":"exec","task_kind":"implement","task_prompt":"Write the Foo struct with bar() method in src/foo.rs","deps":[]}},
  {{"agent_id":"doc_writer","task_kind":"document","task_prompt":"Add doc-comments to src/foo.rs","deps":[0]}}
]

Respond with the JSON array only. No prose."#,
        goal = goal
    )
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

fn parse_decompose_tasks(response: &str, parent_request_id: &str) -> Vec<RequestDispatch> {
    // Find the JSON array in the response (LLM may include prose before/after).
    let trimmed = response.trim();
    let start = trimmed.find('[').unwrap_or(0);
    let end = trimmed.rfind(']').map(|i| i + 1).unwrap_or(trimmed.len());
    let json_slice = &trimmed[start..end];

    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(json_slice) else {
        return vec![];
    };

    // First pass: generate dispatch IDs so we can resolve index-based deps.
    let ids: Vec<String> = arr.iter().map(|_| Uuid::new_v4().to_string()).collect();

    arr.iter().enumerate().filter_map(|(i, item)| {
        let agent_id = item.get("agent_id")?.as_str()?.to_string();
        let task_kind = item.get("task_kind").and_then(|v| v.as_str()).unwrap_or("task").to_string();
        let task_prompt = item.get("task_prompt")?.as_str()?.to_string();
        let deps: Vec<String> = item.get("deps")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|d| d.as_u64().map(|idx| idx as usize))
                    .filter(|&idx| idx < ids.len() && idx != i)
                    .map(|idx| ids[idx].clone())
                    .collect()
            })
            .unwrap_or_default();

        Some(RequestDispatch {
            dispatch_id: ids[i].clone(),
            parent_request_id: parent_request_id.to_string(),
            agent_id,
            task_prompt,
            task_kind,
            deps,
            workspace_scope: None,
        })
    }).collect()
}

fn fallback_dispatch(goal: &str, parent_request_id: &str) -> RequestDispatch {
    RequestDispatch {
        dispatch_id: Uuid::new_v4().to_string(),
        parent_request_id: parent_request_id.to_string(),
        agent_id: "exec".to_string(),
        task_prompt: goal.to_string(),
        task_kind: "implement".to_string(),
        deps: vec![],
        workspace_scope: None,
    }
}
