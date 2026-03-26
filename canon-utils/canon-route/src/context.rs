use canon_decision::JournalLine;
use canon_event::{RuntimeEvent, LoopActed, LoopObserved, LoopPlanned, LoopRewarded, LoopVerified, ToolCall, ToolResult, SubTaskResult};
use canon_goal::{parse_agent_goal_markdown, summarize_goal, GoalSpec};
use crate::causal::update_causal_graph;
use canon_judgment::{LlmSignals, RuntimeSignals};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Default, Clone)]
pub struct WorkspaceDirtyTracker {
    dirty_by_agent: HashMap<String, Vec<String>>,
}

impl WorkspaceDirtyTracker {
    pub fn mark_dirty(&mut self, agent: &str, action_id: Option<&str>) {
        let entry = self.dirty_by_agent.entry(agent.to_string()).or_default();
        if let Some(a) = action_id {
            entry.push(a.to_string());
        }
    }
    pub fn mark_verified(&mut self, agent: &str) {
        self.dirty_by_agent.remove(agent);
    }
    pub fn any_dirty(&self) -> bool {
        !self.dirty_by_agent.is_empty()
    }
    pub fn all_clean(&self) -> bool {
        self.dirty_by_agent.is_empty()
    }
}

#[derive(Default)]
pub struct RouteContext {
    pub scheduler_tick: u64,
    pub mission_raw: String,
    pub mission_summary: String,
    pub mission_goal_spec: Option<GoalSpec>,
    pub context_ready: bool,
    pub workspace_dirty_tracker: WorkspaceDirtyTracker,
    pub planned_pending: usize,
    pub acted_unverified: bool,
    pub last_action_failed: bool,
    pub pending_tool_result_ids: HashSet<String>,
    pub recent_tool_results: Vec<serde_json::Value>,
    pub finish_ready: bool,
    pub last_action_kind: String,
    pub journal: Vec<JournalLine>,
    pub last_llm_signals: Option<serde_json::Value>,
    pub halted: bool,
    pub goodness: Option<f32>,
    pub delta_g: Option<f32>,
    /// Set to Some(...) when the last pending tool result lands; cleared after the executor emits ToolBatchSettled.
    pub batch_settled: Option<(u32, bool)>, // (result_count, any_failed)
    batch_result_count: u32,
    batch_any_failed: bool,
    /// True when the current batch contains only llm.plan calls — routing deferred to LoopPlanned.
    pub batch_is_plan_only: bool,
    /// Maps action_id → (action_kind, llm_request_id) for enriching ToolResult metadata.
    action_meta: HashMap<String, (String, Option<String>)>,
    pub causal_graph: crate::causal::CausalGraph,
}

impl RouteContext {
    pub fn new() -> Self {
        Self::default()
    }

    fn goal_is_placeholder(goal: &str) -> bool {
        let trimmed = goal.trim();
        trimmed.is_empty() || trimmed.contains("goal-pending")
    }

    pub fn signals(&self) -> RuntimeSignals {
        RuntimeSignals {
            context_ready: self.context_ready,
            has_queued_plan: self.planned_pending > 0,
            workspace_dirty: self.workspace_dirty_tracker.any_dirty(),
            performed_recently: self.acted_unverified,
            last_action_failed: self.last_action_failed,
            finish_ready: self.finish_ready && self.workspace_dirty_tracker.all_clean(),
            last_action_kind: self.last_action_kind.clone(),
            llm_signals: self.last_llm_signals.as_ref().map(LlmSignals::from_value),
            goodness: self.goodness,
            delta_g: self.delta_g,
        }
    }

    pub fn snapshot_text(&self) -> String {
        format!(
            "tick={tick}\ncontext_ready={context}\nworkspace_dirty={dirty}\nplanned_pending={pending}\nacted_unverified={unverified}\nfinish_ready={finish}\nlast_action_kind={action}\ngoodness={goodness}\ndelta_g={delta_g}",
            tick = self.scheduler_tick,
            context = self.context_ready,
            dirty = self.workspace_dirty_tracker.any_dirty(),
            pending = self.planned_pending,
            unverified = self.acted_unverified,
            finish = self.finish_ready,
            action = self.last_action_kind,
            goodness = self.goodness.map(|v| v.to_string()).unwrap_or_else(|| "NA".into()),
            delta_g = self.delta_g.map(|v| v.to_string()).unwrap_or_else(|| "NA".into()),
        )
    }

    pub fn push_journal(&mut self, lane: impl Into<String>, summary: impl Into<String>) {
        self.journal.push(JournalLine { lane: lane.into(), summary: summary.into(), data: serde_json::Value::Null });
        if self.journal.len() > 32 {
            let drop_n = self.journal.len() - 32;
            self.journal.drain(0..drop_n);
        }
    }

    pub fn update_from_event(&mut self, event: &RuntimeEvent, workspace: &Path) {
        match event {
            RuntimeEvent::LoopObserved(LoopObserved { goal_text, error_count, .. }) => {
                let goal_present = goal_text
                    .as_ref()
                    .map(|v| !Self::goal_is_placeholder(v))
                    .unwrap_or(false);
                self.context_ready = goal_present || *error_count > 0;
                if let Some(goal_text) = goal_text {
                    if !Self::goal_is_placeholder(goal_text) {
                        self.mission_raw = goal_text.clone();
                        self.mission_summary = summarize_goal(&parse_agent_goal_markdown(goal_text));
                        self.mission_goal_spec = Some(parse_agent_goal_markdown(goal_text));
                    }
                }
                self.push_journal("observe", format!("tick={} goal_present={} errors={}", self.scheduler_tick, goal_present, error_count));
            }
            RuntimeEvent::LoopPlanned(LoopPlanned { action_kind, plan_id, action_id, llm_request_id, signals, .. }) => {
                if action_kind != "no_op" {
                    self.planned_pending = self.planned_pending.saturating_add(1);
                }
                update_causal_graph(&mut self.causal_graph, event);
                if let Some(sig) = signals {
                    self.last_llm_signals = Some(sig.clone());
                }
                // Record action_id → (action_kind, llm_request_id) for ToolResult enrichment.
                if let Some(aid) = action_id {
                    self.action_meta.insert(aid.clone(), (action_kind.clone(), llm_request_id.clone()));
                }
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
            RuntimeEvent::LoopActed(LoopActed { action_kind, capability_request_id, tool_call_id, tool_result_id, success, stderr, action_id, .. }) => {
                self.planned_pending = self.planned_pending.saturating_sub(1);
                update_causal_graph(&mut self.causal_graph, event);
                // Only mark dirty/acted_unverified for mutating actions.
                const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files", "done"];
                if !READ_ONLY_ACTIONS.contains(&action_kind.as_str()) {
                    self.acted_unverified = true;
                    self.workspace_dirty_tracker.mark_dirty("orchestrator", action_id.as_deref());
                }
                if stderr != "skipped:batch_aborted" {
                    self.last_action_failed = !success;
                }
                if let Some(tool_call_id) = tool_call_id {
                    if tool_result_id.is_some() {
                        self.pending_tool_result_ids.remove(tool_call_id);
                    }
                }
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
            RuntimeEvent::LoopVerified(LoopVerified { compiler_clean, passed, diagnostics, .. }) => {
                self.acted_unverified = false;
                self.workspace_dirty_tracker.mark_verified("orchestrator");
                let done_action = self.last_action_kind == "done";
                let system_satisfied = done_action && *passed
                    || crate::helpers::evaluate_goal_satisfied(self.mission_goal_spec.as_ref(), workspace);
                self.finish_ready = *compiler_clean && system_satisfied;
                self.push_journal("verify", format!("passed={} done_action={done_action} system_satisfied={} diagnostics={}", compiler_clean, system_satisfied, diagnostics.join("|")));
            }
            RuntimeEvent::LoopRewarded(LoopRewarded { halt, .. }) => {
                if *halt {
                    self.halted = true;
                }
                self.push_journal("reward", format!("halt={halt}"));
            }
            RuntimeEvent::SubTaskResult(SubTaskResult { agent_id, success, .. }) => {
                // Treat sub-task results with writes as dirty until reconcile; conservative: any sub-task success toggles acted_unverified.
                self.workspace_dirty_tracker.mark_dirty(agent_id, None);
                if *success {
                    self.acted_unverified = true;
                }
                update_causal_graph(&mut self.causal_graph, event);
            }
            RuntimeEvent::RequestDispatch(_) => {
                update_causal_graph(&mut self.causal_graph, event);
            }
            RuntimeEvent::ToolCall(ToolCall { tool_call_id, kind, .. }) => {
                // Opening a new call: if set was empty this starts a new batch.
                if self.pending_tool_result_ids.is_empty() {
                    self.batch_result_count = 0;
                    self.batch_any_failed = false;
                    self.batch_settled = None;
                    self.batch_is_plan_only = kind == "llm.plan";
                } else if kind != "llm.plan" {
                    self.batch_is_plan_only = false;
                }
                self.pending_tool_result_ids.insert(tool_call_id.clone());
                update_causal_graph(&mut self.causal_graph, event);
            }
            RuntimeEvent::ToolResult(ToolResult { node_id, kind, success, request_id, tool_call_id, tool_result_id, output, .. }) => {
                self.pending_tool_result_ids.remove(tool_call_id);
                self.batch_result_count += 1;
                if !success {
                    self.batch_any_failed = true;
                }
                update_causal_graph(&mut self.causal_graph, event);
                let mut output_text = output.to_string();
                if output_text.len() > 512 {
                    output_text.truncate(512);
                    output_text.push_str("...<truncated>");
                }
                let (action_kind, llm_request_id) = self.action_meta.get(node_id)
                    .cloned()
                    .unwrap_or_default();
                self.recent_tool_results.push(json!({
                    "node_id": node_id,
                    "kind": kind,
                    "action": action_kind,
                    "llm_request_id": llm_request_id,
                    "success": success,
                    "request_id": request_id,
                    "tool_call_id": tool_call_id,
                    "tool_result_id": tool_result_id,
                    "output": output,
                }));
                if self.recent_tool_results.len() > 8 {
                    self.recent_tool_results.remove(0);
                }
                // If all pending calls have now resolved, mark the batch as settled.
                // Skip for plan-only batches — routing is deferred to LoopPlanned so that
                // planned_pending is already updated when the route is selected.
                if self.pending_tool_result_ids.is_empty() && self.planned_pending == 0 && !self.batch_is_plan_only {
                    self.batch_settled = Some((self.batch_result_count, self.batch_any_failed));
                }
                self.push_journal("tool", format!("tool_result kind={kind} success={success} tool_call_id={tool_call_id} tool_result_id={tool_result_id} output={output_text}"));
            }
            RuntimeEvent::RuntimeStateUpdated(updated) => {
                if updated.payload.get("fatal_invariant").and_then(|v| v.as_bool()).unwrap_or(false) {
                    self.halted = true;
                    if let Some(reason) = updated.payload.get("fatal_invariant_reason").and_then(|v| v.as_str()) {
                        self.push_journal("runtime", format!("fatal_invariant_halt reason={reason}"));
                    }
                } else if updated.payload.get("runtime_mode").and_then(|v| v.as_str()) == Some("running") {
                    self.halted = false;
                    self.push_journal("runtime", "mode=running");
                }
                let dirty = updated.payload.get("workspace_dirty").and_then(|v| v.as_bool()).unwrap_or(false);
                if dirty {
                    self.workspace_dirty_tracker.mark_dirty("orchestrator", None);
                    let crate_name = updated.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                    self.push_journal("observe", format!("workspace_dirty=true crate={crate_name}"));
                }
            }
            RuntimeEvent::GoodnessSnapshot(g) => {
                self.goodness = Some(g.g);
                self.delta_g = Some(g.delta_g);
                self.push_journal("goodness", format!("g={} delta={}", g.g, g.delta_g));
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
            &RuntimeEvent::LoopObserved(LoopObserved {
                tick: 1,
                error_count: 0,
                warning_count: 0,
                compiler_errors: Vec::new(),
                goal_text: Some("goal".into()),
                workspace_facts: Vec::new(),
            }),
            workspace,
        );
        assert!(ctx.context_ready);
        ctx.update_from_event(
            &RuntimeEvent::LoopPlanned(LoopPlanned {
                tick: 1,
                action_kind: "act".into(),
                action_payload: json!({}),
                reason: "r".into(),
                llm_request_id: Some("req".into()),
                signals: None,
                trace_id: None,
                execution_id: None,
                span_id: None,
                parent_span_id: None,
                plan_id: None,
                plan_step_id: None,
                action_id: None,
                depends_on: vec![],
            }),
            workspace,
        );
        assert_eq!(ctx.planned_pending, 1);
        ctx.update_from_event(
            &RuntimeEvent::LoopActed(LoopActed {
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
