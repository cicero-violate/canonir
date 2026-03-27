use canon_decision::JournalLine;
use canon_event::{RuntimeEvent, LoopActed, LoopObserved, LoopPlanned, LoopRewarded, LoopVerified, ToolCall, ToolResult, SubTaskResult};
use canon_goal::{parse_agent_goal_markdown, summarize_goal, GoalSpec};
use canon_semantic_state::{
    classify_planned_action_intents, derive_objective_trend_state, derive_self_development_objective_state,
    execution_results_for_action, LlmSemanticContext, ObjectiveTrendState, SemanticActionIntent,
    SemanticExecutionResultRecord, SemanticStateSummary, primary_development_objective_kind,
    semantic_no_progress_streak, semantic_progress_rate,
};
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
    pub pending_tool_result_ids: HashSet<String>,
    pub recent_tool_results: Vec<serde_json::Value>,
    pub finish_ready: bool,
    pub last_action_kind: String,
    pub verify_seen: bool,
    pub last_verify_passed: bool,
    pub last_verify_compiler_clean: bool,
    pub last_verify_diagnostics: Vec<String>,
    pub journal: Vec<JournalLine>,
    pub last_llm_signals: Option<serde_json::Value>,
    pub halted: bool,
    pub goodness: Option<f32>,
    pub delta_g: Option<f32>,
    pub objective_trend_state: ObjectiveTrendState,
    /// Set to Some(...) when the last pending tool result lands; cleared after the executor emits ToolBatchSettled.
    pub batch_settled: Option<(u32, bool)>, // (result_count, any_failed)
    batch_result_count: u32,
    batch_any_failed: bool,
    /// True when the current batch contains only llm.plan calls — routing deferred to LoopPlanned.
    pub batch_is_plan_only: bool,
    pub last_invalid_plan_reason: Option<String>,
    pub last_invalid_plan_planned_count: Option<usize>,
    pub consecutive_invalid_plan_batches: u32,
    pub bootstrap_refresh_required: bool,
    pub semantic_summary: SemanticStateSummary,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
    pub last_halt_reason: Option<String>,
    /// Maps action_id → (action_kind, llm_request_id) for enriching ToolResult metadata.
    action_meta: HashMap<String, (String, Option<String>)>,
    action_semantic_intents: HashMap<String, Vec<SemanticActionIntent>>,
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
            performed_recently: self.acted_unverified,
            repair_stalled: semantic_no_progress_streak(&self.recent_execution_results) > 0,
            repair_pressure_score: self.objective_state().repair_pressure_score(),
            finish_ready: self.finish_ready && self.workspace_dirty_tracker.all_clean(),
            last_action_kind: self.last_action_kind.clone(),
            llm_signals: self.last_llm_signals.as_ref().map(LlmSignals::from_value),
            goodness: self.goodness,
            delta_g: self.delta_g,
        }
    }

    pub fn target_workspace_missing_state(&self) -> bool {
        semantic_has_target_state(&self.semantic_summary) && !self.semantic_summary.path_exists
    }

    pub fn target_workspace_path_state(&self) -> Option<&str> {
        self.semantic_summary.target_root.as_deref()
    }

    pub fn planning_preconditions_state(&self) -> &[String] {
        &self.semantic_summary.planning_preconditions
    }

    pub fn validation_blocked_state(&self) -> bool {
        self.semantic_summary.validation_blocked_by_preconditions
    }

    pub fn compiler_repair_required_state(&self) -> bool {
        self.semantic_summary.compiler_repair_required
    }

    pub fn snapshot_text(&self) -> String {
        let objective_state = self.objective_state();
        let primary_objective = primary_development_objective_kind(
            &objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        format!(
            "tick={tick}\ncontext_ready={context}\nworkspace_dirty={dirty}\nplanned_pending={pending}\nacted_unverified={unverified}\nfinish_ready={finish}\nlast_action_kind={action}\ngoodness={goodness}\ndelta_g={delta_g}\nconsecutive_invalid_plan_batches={invalid_count}\nlast_invalid_plan_planned_count={invalid_planned}\nlast_invalid_plan_reason={invalid_reason}\ntarget_workspace_missing={target_missing}\ntarget_workspace_path={target_path}\nplanning_preconditions={planning_preconditions}\nvalidation_blocked_by_preconditions={validation_blocked}\ncompiler_repair_required={compiler_repair}\nprimary_development_objective={primary_objective}\nmisalignment_pressure_score={misalignment_pressure}\nsemantic_summary_version={semantic_version}\nsemantic_summary_complete={semantic_complete}\nsemantic_progress_rate={semantic_progress_rate:.2}\nsemantic_no_progress_streak={semantic_no_progress_streak}\nhalted={halted}\nlast_halt_reason={halt_reason}",
            tick = self.scheduler_tick,
            context = self.context_ready,
            dirty = self.workspace_dirty_tracker.any_dirty(),
            pending = self.planned_pending,
            unverified = self.acted_unverified,
            finish = self.finish_ready,
            action = self.last_action_kind,
            goodness = self.goodness.map(|v| v.to_string()).unwrap_or_else(|| "NA".into()),
            delta_g = self.delta_g.map(|v| v.to_string()).unwrap_or_else(|| "NA".into()),
            invalid_count = self.consecutive_invalid_plan_batches,
            invalid_planned = self
                .last_invalid_plan_planned_count
                .map(|v| v.to_string())
                .unwrap_or_else(|| "NA".into()),
            invalid_reason = self
                .last_invalid_plan_reason
                .as_deref()
                .unwrap_or("NA"),
            target_missing = self.target_workspace_missing_state(),
            target_path = self.target_workspace_path_state().unwrap_or("NA"),
            planning_preconditions = if self.planning_preconditions_state().is_empty() {
                "NA".to_string()
            } else {
                self.planning_preconditions_state().join("|")
            },
            validation_blocked = self.validation_blocked_state(),
            compiler_repair = self.compiler_repair_required_state(),
            primary_objective = primary_objective.as_str(),
            misalignment_pressure = objective_state.misalignment_pressure_score,
            semantic_version = self.semantic_summary.version,
            semantic_complete = self.semantic_summary.complete,
            semantic_progress_rate = semantic_progress_rate(&self.recent_execution_results),
            semantic_no_progress_streak = semantic_no_progress_streak(&self.recent_execution_results),
            halted = self.halted,
            halt_reason = self.last_halt_reason.as_deref().unwrap_or("NA"),
        )
    }

    pub fn llm_semantic_context(&self) -> LlmSemanticContext {
        let objective_state = derive_self_development_objective_state(
            &self.semantic_summary,
            self.consecutive_invalid_plan_batches,
            &self.recent_execution_results,
            &self.objective_trend_state,
        );
        LlmSemanticContext {
            mission_summary: if self.mission_summary.is_empty() {
                None
            } else {
                Some(self.mission_summary.clone())
            },
            semantic_summary: self.semantic_summary.clone(),
            objective_state,
            objective_trend_state: self.objective_trend_state.clone(),
            target_workspace: self.semantic_summary.target_root.clone(),
            workspace_loc: None,
            error_count: None,
            warning_count: None,
            route_rationale: None,
            route_confidence: None,
            invalid_plan_reason: self.last_invalid_plan_reason.clone(),
            invalid_plan_planned_count: self.last_invalid_plan_planned_count,
            consecutive_invalid_plan_batches: self.consecutive_invalid_plan_batches,
            low_level_diagnostics: self.last_verify_diagnostics.clone(),
            recent_actions: Vec::new(),
            recent_tool_results: Vec::new(),
            recent_execution_results: self.recent_execution_results.clone(),
        }
    }

    pub fn objective_state(&self) -> canon_semantic_state::SelfDevelopmentObjectiveState {
        derive_self_development_objective_state(
            &self.semantic_summary,
            self.consecutive_invalid_plan_batches,
            &self.recent_execution_results,
            &self.objective_trend_state,
        )
    }

    pub fn refresh_objective_trend_state(&mut self) {
        self.objective_trend_state = derive_objective_trend_state(
            self.objective_trend_state.planning_attempts,
            self.objective_trend_state.invalid_plan_events,
            self.goodness,
            self.delta_g,
            &self.recent_execution_results,
        );
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
            RuntimeEvent::RustcGraphArtifactWritten(written) => {
                self.semantic_summary.apply_graph_artifact_summary(
                    written.artifact_id.clone(),
                    written.node_count as usize,
                    written.edge_count as usize,
                    written.file_count as usize,
                    written.call_edge_count as usize,
                    written.module_edge_count as usize,
                    written.cfg_edge_count as usize,
                );
                self.push_journal(
                    "observe",
                    format!(
                        "rustc_graph_artifact_written crate={} artifact_id={} files={} nodes={} edges={}",
                        written.crate_name,
                        written.artifact_id,
                        written.file_count,
                        written.node_count,
                        written.edge_count
                    ),
                );
            }
            RuntimeEvent::RustcCaptureCompleted(completed) => {
                self.push_journal(
                    "observe",
                    format!(
                        "rustc_capture_completed crate={} artifact_id={}",
                        completed.crate_name, completed.artifact_id
                    ),
                );
            }
            RuntimeEvent::RustcCaptureFailed(failed) => {
                self.push_journal(
                    "observe",
                    format!("rustc_capture_failed crate={} message={}", failed.crate_name, failed.message),
                );
            }
            RuntimeEvent::LoopObserved(LoopObserved { goal_text, error_count, semantic_summary, .. }) => {
                let goal_present = goal_text
                    .as_ref()
                    .map(|v| !Self::goal_is_placeholder(v))
                    .unwrap_or(false);
                self.context_ready = goal_present || *error_count > 0;
                self.bootstrap_refresh_required = false;
                self.semantic_summary = semantic_summary.clone();
                self.objective_trend_state
                    .record_observation(*error_count, &self.semantic_summary);
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
                    let target_root = self.semantic_summary.target_root.as_deref().map(Path::new);
                    let intents = match event {
                        RuntimeEvent::LoopPlanned(planned) => {
                            classify_planned_action_intents(&planned.action_kind, &planned.action_payload, target_root)
                        }
                        _ => Vec::new(),
                    };
                    self.action_semantic_intents.insert(aid.clone(), intents);
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
                const READ_ONLY_ACTIONS: &[&str] = &["list_dir", "read_file", "search_files", "done"];
                if !READ_ONLY_ACTIONS.contains(&action_kind.as_str()) {
                    self.consecutive_invalid_plan_batches = 0;
                    self.last_invalid_plan_reason = None;
                    self.last_invalid_plan_planned_count = None;
                }
                update_causal_graph(&mut self.causal_graph, event);
                // Only mark dirty/acted_unverified for mutating actions.
                if !READ_ONLY_ACTIONS.contains(&action_kind.as_str()) {
                    self.acted_unverified = true;
                    self.workspace_dirty_tracker.mark_dirty("orchestrator", action_id.as_deref());
                }
                if let Some(tool_call_id) = tool_call_id {
                    if tool_result_id.is_some() {
                        self.pending_tool_result_ids.remove(tool_call_id);
                    }
                }
                if let Some(action_id) = action_id {
                    if let Some(intents) = self.action_semantic_intents.remove(action_id) {
                        let results = execution_results_for_action(&intents, *success, stderr);
                        self.objective_trend_state.record_execution_results(&results);
                        self.recent_execution_results.extend(results);
                        if self.recent_execution_results.len() > 16 {
                            let drop_n = self.recent_execution_results.len() - 16;
                            self.recent_execution_results.drain(0..drop_n);
                        }
                    }
                }
                if is_successful_bootstrap(action_kind, *success, stderr) {
                    self.bootstrap_refresh_required = true;
                    self.planned_pending = 0;
                    self.semantic_summary.path_exists = true;
                    self.semantic_summary.planning_preconditions.clear();
                    self.semantic_summary.validation_blocked_by_preconditions = false;
                    self.semantic_summary.compiler_repair_required = false;
                    self.semantic_summary.repair_intents.clear();
                    self.semantic_summary.compiler_hints.clear();
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
                self.verify_seen = true;
                self.last_verify_passed = *passed;
                self.last_verify_compiler_clean = *compiler_clean;
                self.last_verify_diagnostics = diagnostics.clone();
                self.semantic_summary.compiler_repair_required = diagnostics.iter().any(|d| {
                    d.contains("allow(dead_code) incompatible with previous forbid")
                        || d.contains("file not found for module `")
                });
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
                    self.last_halt_reason = Some("loop_rewarded requested halt".to_string());
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
            RuntimeEvent::ErrorOccurred(err) if err.kind == "invalid_plan_batch" => {
                self.consecutive_invalid_plan_batches = self.consecutive_invalid_plan_batches.saturating_add(1);
                self.objective_trend_state.record_invalid_plan_event();
                self.last_invalid_plan_reason = Some(err.message.clone());
                self.last_invalid_plan_planned_count = err
                    .context
                    .get("planned_count")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                self.push_journal(
                    "plan",
                    format!(
                        "invalid_plan_batch count={} planned_count={} reason={}",
                        self.consecutive_invalid_plan_batches,
                        self.last_invalid_plan_planned_count
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "NA".to_string()),
                        err.message
                    ),
                );
            }
            RuntimeEvent::PlanningCompleted(pc) if pc.status != "invalid_plan" => {
                self.objective_trend_state.record_planning_completion(&pc.status);
                self.consecutive_invalid_plan_batches = 0;
                self.last_invalid_plan_reason = None;
                self.last_invalid_plan_planned_count = None;
            }
            RuntimeEvent::PlanningCompleted(pc) => {
                self.objective_trend_state.record_planning_completion(&pc.status);
            }
            RuntimeEvent::Debug(debug) if debug.kind == "bootstrap_refresh_required" => {
                self.bootstrap_refresh_required = true;
                self.planned_pending = 0;
                self.semantic_summary.path_exists = true;
                self.push_journal("observe", "bootstrap_refresh_required");
            }
            RuntimeEvent::Debug(debug) if debug.kind == "route_objective_contradiction" => {
                self.objective_trend_state.record_route_objective_contradiction();
                self.push_journal("route", "route_objective_contradiction");
            }
            RuntimeEvent::Debug(debug) if debug.kind == "goal_objective_drift" => {
                self.objective_trend_state.record_goal_objective_drift();
                self.push_journal("goal", "goal_objective_drift");
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
                    self.last_halt_reason = updated
                        .payload
                        .get("fatal_invariant_reason")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string());
                    if let Some(reason) = updated.payload.get("fatal_invariant_reason").and_then(|v| v.as_str()) {
                        self.push_journal("runtime", format!("fatal_invariant_halt reason={reason}"));
                    }
                } else if updated.payload.get("runtime_mode").and_then(|v| v.as_str()) == Some("running") {
                    self.halted = false;
                    self.last_halt_reason = None;
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
                self.objective_trend_state.record_goodness(g.g, g.delta_g);
                self.push_journal("goodness", format!("g={} delta={}", g.g, g.delta_g));
            }
            _ => {}
        }
    }
}

fn semantic_has_target_state(summary: &SemanticStateSummary) -> bool {
    summary.complete || summary.target_root.is_some()
}

fn is_successful_bootstrap(action_kind: &str, success: bool, stderr: &str) -> bool {
    success
        && action_kind == "run_command"
        && (stderr.contains("Creating binary (application) package")
            || stderr.contains("Creating library package")
            || stderr.contains("Creating binary (application) `")
            || stderr.contains("Creating library `"))
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
                semantic_summary: SemanticStateSummary::default(),
                observe_diagnostics: Vec::new(),
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

    #[test]
    fn read_only_actions_preserve_invalid_plan_memory() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");
        ctx.consecutive_invalid_plan_batches = 2;
        ctx.last_invalid_plan_reason = Some("invalid hunk at line 12".into());
        ctx.last_invalid_plan_planned_count = Some(3);

        ctx.update_from_event(
            &RuntimeEvent::LoopActed(LoopActed {
                tick: 1,
                action_kind: "read_file".into(),
                capability_request_id: "req".into(),
                tool_call_id: None,
                tool_result_id: None,
                stdout: "contents".into(),
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

        assert_eq!(ctx.consecutive_invalid_plan_batches, 2);
        assert_eq!(ctx.last_invalid_plan_reason.as_deref(), Some("invalid hunk at line 12"));
        assert_eq!(ctx.last_invalid_plan_planned_count, Some(3));
    }

    #[test]
    fn bootstrap_action_clears_missing_target_and_requests_refresh() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");
        ctx.semantic_summary.target_root = Some("/tmp/example".into());
        ctx.semantic_summary.path_exists = false;
        ctx.planned_pending = 2;

        ctx.update_from_event(
            &RuntimeEvent::LoopActed(LoopActed {
                tick: 1,
                action_kind: "run_command".into(),
                capability_request_id: "req".into(),
                tool_call_id: None,
                tool_result_id: None,
                stdout: String::new(),
                stderr: "    Creating binary (application) package".into(),
                exit_code: Some(0),
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

        assert!(!ctx.target_workspace_missing_state());
        assert!(ctx.bootstrap_refresh_required);
        assert_eq!(ctx.planned_pending, 0);
    }

    #[test]
    fn observe_facts_capture_planning_preconditions() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");
        ctx.update_from_event(
            &RuntimeEvent::LoopObserved(LoopObserved {
                tick: 1,
                error_count: 0,
                warning_count: 0,
                compiler_errors: Vec::new(),
                goal_text: Some("goal".into()),
                semantic_summary: SemanticStateSummary {
                    version: SemanticStateSummary::VERSION,
                    complete: true,
                    planning_preconditions: vec![
                        "must_create_entrypoint=true repair=create_src_main_or_lib_before_cargo_check".into(),
                        "must_fix_dead_code_forbid_conflict=true repair=remove_allow_dead_code_or_make_code_used".into(),
                    ],
                    validation_blocked_by_preconditions: true,
                    compiler_repair_required: true,
                    ..SemanticStateSummary::default()
                },
                observe_diagnostics: Vec::new(),
            }),
            workspace,
        );

        assert!(ctx.validation_blocked_state());
        assert!(ctx.compiler_repair_required_state());
        assert_eq!(ctx.planning_preconditions_state().len(), 2);
    }

    #[test]
    fn semantic_observation_payload_is_consumed() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");
        ctx.update_from_event(
            &RuntimeEvent::LoopObserved(LoopObserved {
                tick: 1,
                error_count: 0,
                warning_count: 0,
                compiler_errors: Vec::new(),
                goal_text: Some("goal".into()),
                semantic_summary: SemanticStateSummary {
                    version: SemanticStateSummary::VERSION,
                    complete: true,
                    target_root: Some("/tmp/example".into()),
                    path_exists: false,
                    planning_preconditions: vec![
                        "must_bootstrap_workspace=true repair=cargo_init_or_create_workspace".into(),
                    ],
                    validation_blocked_by_preconditions: true,
                    compiler_repair_required: false,
                    ..SemanticStateSummary::default()
                },
                observe_diagnostics: Vec::new(),
            }),
            workspace,
        );
        assert!(ctx.target_workspace_missing_state());
        assert_eq!(ctx.target_workspace_path_state(), Some("/tmp/example"));
        assert!(ctx.validation_blocked_state());
        assert_eq!(ctx.planning_preconditions_state().len(), 1);
    }

    #[test]
    fn semantic_state_helpers_prefer_summary_when_complete() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.target_root = Some("/tmp/semantic".into());
        ctx.semantic_summary.planning_preconditions =
            vec!["must_create_missing_modules=true".into()];
        ctx.semantic_summary.validation_blocked_by_preconditions = true;
        ctx.semantic_summary.compiler_repair_required = true;

        assert!(ctx.target_workspace_missing_state());
        assert_eq!(ctx.target_workspace_path_state(), Some("/tmp/semantic"));
        assert!(ctx.validation_blocked_state());
        assert!(ctx.compiler_repair_required_state());
        assert_eq!(ctx.planning_preconditions_state().len(), 1);
    }

    #[test]
    fn contradiction_debug_events_update_objective_trends() {
        let mut ctx = RouteContext::default();
        let workspace = Path::new("/tmp");

        ctx.update_from_event(
            &RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "route_executor".into(),
                kind: "route_objective_contradiction".into(),
                payload: serde_json::json!({}),
            }),
            workspace,
        );
        ctx.update_from_event(
            &RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "goal_gen_consumer".into(),
                kind: "goal_objective_drift".into(),
                payload: serde_json::json!({}),
            }),
            workspace,
        );

        assert_eq!(ctx.objective_trend_state.route_objective_contradiction_events, 1);
        assert_eq!(ctx.objective_trend_state.goal_objective_drift_events, 1);
    }
}
