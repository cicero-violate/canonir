use canon_decision::JournalLine;
use canon_event::{CanonEvent, LoopActed, LoopObserved, LoopPlanned, LoopRewarded, LoopVerified, ToolCall, ToolResult};
use canon_goal::{parse_agent_goal_markdown, summarize_goal, GoalSpec};
use canon_judgment::RuntimeSignals;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;

#[derive(Default)]
pub struct RouteContext {
    pub scheduler_tick: u64,
    pub mission_raw: String,
    pub mission_summary: String,
    pub mission_goal_spec: Option<GoalSpec>,
    pub context_ready: bool,
    pub workspace_dirty: bool,
    pub planned_pending: usize,
    pub acted_unverified: bool,
    pub last_action_failed: bool,
    pub pending_tool_result_ids: HashSet<String>,
    pub latest_tool_result: Option<serde_json::Value>,
    pub finish_ready: bool,
    pub last_action_kind: String,
    pub journal: Vec<JournalLine>,
}

impl RouteContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn signals(&self) -> RuntimeSignals {
        RuntimeSignals {
            context_ready: self.context_ready,
            has_queued_plan: self.planned_pending > 0,
            workspace_dirty: self.workspace_dirty,
            performed_recently: self.acted_unverified,
            last_action_failed: self.last_action_failed,
            finish_ready: self.finish_ready,
        }
    }

    pub fn snapshot_text(&self) -> String {
        format!(
            "tick={tick}\ncontext_ready={context}\nworkspace_dirty={dirty}\nplanned_pending={pending}\nacted_unverified={unverified}\nfinish_ready={finish}\nlast_action_kind={action}",
            tick = self.scheduler_tick,
            context = self.context_ready,
            dirty = self.workspace_dirty,
            pending = self.planned_pending,
            unverified = self.acted_unverified,
            finish = self.finish_ready,
            action = self.last_action_kind,
        )
    }

    pub fn push_journal(&mut self, lane: impl Into<String>, summary: impl Into<String>) {
        self.journal.push(JournalLine { lane: lane.into(), summary: summary.into(), data: serde_json::Value::Null });
        if self.journal.len() > 32 {
            let drop_n = self.journal.len() - 32;
            self.journal.drain(0..drop_n);
        }
    }

    pub fn update_from_event(&mut self, event: &CanonEvent, workspace: &Path) {
        match event {
            CanonEvent::LoopObserved(LoopObserved { goal_text, error_count, .. }) => {
                let goal_present = goal_text.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false);
                self.context_ready = goal_present || *error_count > 0;
                if let Some(goal_text) = goal_text {
                    if !goal_text.trim().is_empty() {
                        self.mission_raw = goal_text.clone();
                        self.mission_summary = summarize_goal(&parse_agent_goal_markdown(goal_text));
                        self.mission_goal_spec = Some(parse_agent_goal_markdown(goal_text));
                    }
                }
                self.push_journal("observe", format!("tick={} goal_present={} errors={}", self.scheduler_tick, goal_present, error_count));
            }
            CanonEvent::LoopPlanned(LoopPlanned { action_kind, plan_id, action_id, llm_request_id, .. }) => {
                self.planned_pending = self.planned_pending.saturating_add(1);
                let mut summary = format!("planned action={action_kind}");
                if let Some(plan_id) = plan_id {
                    summary.push_str(&format!(" plan_id={plan_id}"));
                }
                if let Some(action_id) = action_id {
                    summary.push_str(&format!(" action_id={action_id}"));
                }
                if let Some(llm_request_id) = llm_request_id {
                    summary.push_str(&format!(" llm_request_id={llm_request_id}"));
                }
                self.push_journal("plan", summary);
            }
            CanonEvent::LoopActed(LoopActed { action_kind, capability_request_id, tool_call_id, tool_result_id, success, stderr, .. }) => {
                self.planned_pending = self.planned_pending.saturating_sub(1);
                self.acted_unverified = true;
                if stderr != "skipped:batch_aborted" {
                    self.last_action_failed = !success;
                }
                if let Some(tool_call_id) = tool_call_id {
                    if tool_result_id.is_some() {
                        self.pending_tool_result_ids.remove(tool_call_id);
                    }
                }
                self.workspace_dirty = true;
                self.last_action_kind = action_kind.clone();
                let mut summary = format!("executed action={} success={} capability_request_id={}", self.last_action_kind, success, capability_request_id);
                if let Some(tool_call_id) = tool_call_id {
                    summary.push_str(&format!(" tool_call_id={tool_call_id}"));
                }
                if let Some(tool_result_id) = tool_result_id {
                    summary.push_str(&format!(" tool_result_id={tool_result_id}"));
                }
                self.push_journal("act", summary);
            }
            CanonEvent::LoopVerified(LoopVerified { compiler_clean, diagnostics, .. }) => {
                self.acted_unverified = false;
                self.workspace_dirty = false;
                let system_satisfied = crate::helpers::evaluate_goal_satisfied(self.mission_goal_spec.as_ref(), workspace);
                self.finish_ready = *compiler_clean && system_satisfied;
                self.push_journal("verify", format!("passed={} system_satisfied={} diagnostics={}", compiler_clean, system_satisfied, diagnostics.join("|")));
            }
            CanonEvent::LoopRewarded(LoopRewarded { halt, .. }) => {
                if *halt {
                    self.finish_ready = true;
                }
                self.push_journal("reward", format!("halt={halt}"));
            }
            CanonEvent::ToolCall(ToolCall { tool_call_id, .. }) => {
                self.pending_tool_result_ids.insert(tool_call_id.clone());
                self.latest_tool_result = None;
            }
            CanonEvent::ToolResult(ToolResult { node_id, kind, success, request_id, tool_call_id, tool_result_id, output, .. }) => {
                self.pending_tool_result_ids.remove(tool_call_id);
                let mut output_text = output.to_string();
                if output_text.len() > 512 {
                    output_text.truncate(512);
                    output_text.push_str("...<truncated>");
                }
                self.latest_tool_result = Some(json!({
                    "node_id": node_id,
                    "kind": kind,
                    "success": success,
                    "request_id": request_id,
                    "tool_call_id": tool_call_id,
                    "tool_result_id": tool_result_id,
                    "output": output,
                }));
                self.push_journal("tool", format!("tool_result kind={kind} success={success} tool_call_id={tool_call_id} tool_result_id={tool_result_id} output={output_text}"));
            }
            CanonEvent::RuntimeStateUpdated(updated) => {
                let dirty = updated.payload.get("workspace_dirty").and_then(|v| v.as_bool()).unwrap_or(false);
                if dirty {
                    self.workspace_dirty = true;
                    let crate_name = updated.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                    self.push_journal("observe", format!("workspace_dirty=true crate={crate_name}"));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_is_bounded_to_32_lines() {
        let mut ctx = RouteContext::default();
        for i in 0..40 {
            ctx.push_journal("lane", format!("line {i}"));
        }
        assert_eq!(ctx.journal.len(), 32);
        assert!(ctx.journal[0].summary.contains("line 8"));
        assert!(ctx.journal[31].summary.contains("line 39"));
    }

    #[test]
    fn route_state_transitions_after_loop_events() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");
        ctx.update_from_event(
            &CanonEvent::LoopObserved(LoopObserved {
                tick: 1,
                error_count: 0,
                warning_count: 0,
                compiler_errors: Vec::new(),
                goal_text: Some("goal".into()),
            }),
            workspace,
        );
        assert!(ctx.context_ready);
        ctx.update_from_event(
            &CanonEvent::LoopPlanned(LoopPlanned {
                tick: 1,
                action_kind: "act".into(),
                action_payload: json!({}),
                reason: "r".into(),
                llm_request_id: Some("req".into()),
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
            }),
            workspace,
        );
        assert_eq!(ctx.planned_pending, 1);
        ctx.update_from_event(
            &CanonEvent::LoopActed(LoopActed {
                tick: 1,
                action_kind: "act".into(),
                capability_request_id: "req".into(),
                tool_call_id: None,
                tool_result_id: None,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
                duration_ms: 0,
                success: true,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
            }),
            workspace,
        );
        assert_eq!(ctx.planned_pending, 0);
    }
}
