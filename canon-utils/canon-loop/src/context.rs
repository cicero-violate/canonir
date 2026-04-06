use crate::harness_repair::{HarnessRepairState, HarnessRepairTarget};
use crate::merge::{ContextMerger, FileWriteTracker, WorkspaceDirtyTracker};
use crate::scheduler::{DependencyTracker, Scheduler};
use canon_event::{EventEmitterHandle, LoopActed, LoopObserved, LoopPlanned, LoopVerified, ToolResult};
use canon_semantic_state::{DevelopmentObjectiveKind, DevelopmentStrategyKind, ObjectiveTrendState, SemanticActionIntent, SemanticExecutionResultRecord, SemanticStateSummary};
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Default)]
pub struct PendingPlan {
    pub tick: u64,
    pub request_id: String,
    pub dispatched_at_tick: u64,
    pub goal_text: Option<String>,
    pub trace_id: String,
    pub execution_id: String,
    pub span_id: String,
    pub plan_id: String,
    pub plan_tool_call_id: String,
}

#[derive(Clone)]
pub struct PendingAct {
    pub tick: u64,
    pub action_kind: String,
    pub tool_kind: String,
    pub request_id: String,
    pub tool_call_id: String,
    pub node_id: String,
    pub started_at: Instant,
    pub trace_id: Option<String>,
    pub execution_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub plan_id: Option<String>,
    pub plan_step_id: Option<String>,
    pub action_id: Option<String>,
    pub artifact_n: u32,
    pub llm_request_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct BatchStatus {
    pub artifact_n: u32,
    pub planned: usize,
    pub dispatched: usize,
    pub completed_ok: usize,
    pub completed_fail: usize,
}

#[derive(Clone, Copy)]
pub enum DestructiveCmdPolicy {
    Allow,
    Warn,
    Block,
}

impl Default for DestructiveCmdPolicy {
    fn default() -> Self {
        DestructiveCmdPolicy::Block
    }
}

impl DestructiveCmdPolicy {
    pub fn from_env() -> Self {
        match std::env::var("CANON_DESTRUCTIVE_CMD_POLICY").unwrap_or_else(|_| "block".to_string()).to_ascii_lowercase().as_str() {
            "allow" => Self::Allow,
            "warn" => Self::Warn,
            _ => Self::Block,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Block => "block",
        }
    }
}

impl LoopContext {
    /// Build canonical ConstraintState for centralized decision engine
    pub fn to_constraint_state(&self) -> canon_invariant::ConstraintState {
        let semantic = self.last_observed.as_ref().map(|observed| &observed.semantic_summary);
        let failure_class = semantic.and_then(|summary| summary.failure_class.as_deref());
        let failure_scope = semantic.and_then(|summary| summary.failure_scope.as_deref());
        let verify_failed = self.last_verifier_outcome.as_deref().map(|value| value != "passed").unwrap_or(false);
        let semantic_goal_exists = self.goal_text.is_some() || self.last_prompted_goal.is_some();
        let has_plan = semantic
            .map(|summary| {
                !summary.validation_blocked_by_preconditions
                    && summary.planning_preconditions.is_empty()
                    && (semantic_goal_exists
                        || !summary.repair_intents.is_empty()
                        || !summary.module_gaps.is_empty()
                        || summary.has_actionable_compiler_hints())
            })
            .unwrap_or(semantic_goal_exists);

        println!("[STATE DETAIL] goal_text_present={} semantic_observed={}", self.goal_text.is_some(), semantic.is_some());

        eprintln!("[STATE] {}:{} {} has_plan={} (source=semantic_summary|semantic_goal)", file!(), line!(), module_path!(), has_plan);

        canon_invariant::ConstraintState {
            semantic_path_exists: semantic.map(|summary| summary.path_exists).unwrap_or(false),
            semantic_cargo_project: semantic.map(|summary| summary.cargo_project).unwrap_or(false),
            real_path_exists: self.workspace.exists(),
            real_cargo_project: self.workspace.join("Cargo.toml").exists(),
            actionable_failure: semantic
                .map(|summary| {
                    summary.validation_blocked_by_preconditions
                        || summary.compiler_repair_required
                        || !summary.planning_preconditions.is_empty()
                        || !summary.repair_intents.is_empty()
                        || !summary.module_gaps.is_empty()
                        || summary.has_actionable_compiler_hints()
                })
                .unwrap_or(false)
                || verify_failed,
            validation_blocked: semantic
                .map(|summary| summary.validation_blocked_by_preconditions || !summary.planning_preconditions.is_empty())
                .unwrap_or(false),
            entrypoint_missing: semantic.map(|summary| summary.entrypoint_kind.is_none() && summary.cargo_project).unwrap_or(false),
            module_gaps_present: semantic.map(|summary| !summary.module_gaps.is_empty()).unwrap_or(false),
            recent_no_semantic_progress: self.objective_trend_state.current_no_progress_streak > 0,
            failure_class_no_actionable: failure_class == Some("no_actionable_failure"),
            failure_scope_localized: matches!(failure_scope, Some("localized")),
            failure_scope_workspace: matches!(failure_scope, Some("workspace")),
            failure_scope_tooling: matches!(failure_scope, Some("tooling")),
            route_objective_contradiction: false,
            has_plan,
        }
    }
}

pub struct LoopContext {
    pub workspace: PathBuf,
    pub tlog_path: PathBuf,
    pub emitter: EventEmitterHandle,

    // Observe
    pub goal_text: Option<String>,
    pub recent_compiler_errors: Vec<serde_json::Value>,
    pub error_count: usize,
    pub warning_count: usize,

    // Plan
    pub pending_plan: Option<PendingPlan>,
    pub last_llm_signals: Option<serde_json::Value>,
    pub last_observed: Option<LoopObserved>,
    pub last_observed_tick: Option<u64>,
    pub last_handled_observed_hash: Option<u64>,
    pub last_planned_observed_tick: Option<u64>,
    pub last_emitted_plan_hash: Option<u64>,
    pub last_done_goal: Option<String>,
    pub batch_acted: Vec<LoopActed>,
    pub batch_tool_results: Vec<ToolResult>,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
    pub objective_trend_state: ObjectiveTrendState,
    pub last_prompted_goal: Option<String>,
    // System prompt caching — tracks which static system prompt the executor has cached.
    pub last_system_prompt_id: Option<u64>,
    // Context-base caching — tracks the slow-changing context section (GOAL + workspace tree).
    pub last_context_base_id: Option<u64>,
    pub last_delta_hash: Option<u64>,
    pub last_route_rationale: Option<String>,
    pub last_route_confidence: Option<f64>,
    pub last_route_rationale_non_empty: Option<String>,
    pub last_route_confidence_non_empty: Option<f64>,
    pub last_route_selected_tick: Option<u64>,
    pub last_invalid_plan_reason: Option<String>,
    pub last_invalid_plan_planned_count: Option<usize>,
    pub consecutive_invalid_plan_batches: u32,

    // Act
    pub scheduler: Scheduler,
    pub dep_tracker: DependencyTracker,
    pub pending_act: Option<PendingAct>,
    pub artifact_dir: PathBuf,
    pub artifact_counter: u32,
    pub active_batch_llm_request_id: Option<String>,
    pub queued_artifact_index: HashMap<String, u32>,
    pub act_batch_tracker: HashMap<String, BatchStatus>,
    pub action_semantic_intents: HashMap<String, Vec<SemanticActionIntent>>,
    pub last_act_reconcile: Option<Instant>,
    pub destructive_cmd_policy: DestructiveCmdPolicy,
    pub file_write_tracker: FileWriteTracker,
    pub write_paths_by_action: HashMap<String, Vec<PathBuf>>,
    pub dirty_tracker: WorkspaceDirtyTracker,
    pub context_merger: ContextMerger,

    // Decompose
    pub pending_decompose_request_id: Option<String>,
    pub agent_id: Option<String>,

    // Verify
    pub last_act_span_id: Option<String>,
    pub last_acted: Option<LoopActed>,
    pub last_verified: Option<LoopVerified>,
    pub last_verifier_outcome: Option<String>,
    pub last_verifier_retry_policy: Option<String>,
    pub last_verifier_reward_bias: Option<String>,
    pub last_verifier_actionable_failure: Option<bool>,

    // Reward
    pub errors_before: usize,
    pub stagnant_ticks: u32,
    pub last_action_kind: String,
    pub halted: bool,
    pub last_halt_reason: Option<String>,
    pub last_control_kind: Option<String>,
    pub last_control_event_id: Option<String>,
    pub pending_required_successor: Option<String>,
    pub last_reward_trace_id: Option<String>,
    pub last_reward_execution_id: Option<String>,
    pub last_reward_verify_span_id: Option<String>,
    pub goodness: Option<f32>,
    pub delta_g: Option<f32>,
    pub last_observed_error_count: u64,
    pub last_observed_goal_hash: u64,
    pub last_observed_facts_hash: u64,
    pub current_tick: u64,
    pub forced_primary_objective: Option<DevelopmentObjectiveKind>,
    pub forced_primary_strategy: Option<DevelopmentStrategyKind>,
}

impl LoopContext {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf, emitter: EventEmitterHandle) -> Self {
        Self {
            workspace: workspace.clone(),
            tlog_path,
            emitter,
            goal_text: None,
            recent_compiler_errors: Vec::new(),
            error_count: 0,
            warning_count: 0,
            pending_plan: None,
            last_llm_signals: None,
            last_observed: None,
            last_observed_tick: None,
            last_handled_observed_hash: None,
            last_planned_observed_tick: None,
            last_emitted_plan_hash: None,
            last_done_goal: None,
            batch_acted: Vec::new(),
            batch_tool_results: Vec::new(),
            recent_execution_results: Vec::new(),
            objective_trend_state: ObjectiveTrendState::default(),
            last_prompted_goal: None,
            last_system_prompt_id: None,
            last_context_base_id: None,
            last_delta_hash: None,
            last_route_rationale: None,
            last_route_confidence: None,
            last_route_rationale_non_empty: None,
            last_route_confidence_non_empty: None,
            last_route_selected_tick: None,
            last_invalid_plan_reason: None,
            last_invalid_plan_planned_count: None,
            consecutive_invalid_plan_batches: 0,
            // CRITICAL FIX: scheduler must persist across ticks (do NOT reinitialize)
            scheduler: Scheduler::default(),
            dep_tracker: DependencyTracker::default(),
            pending_act: None,
            artifact_dir: default_artifact_dir(&workspace),
            artifact_counter: next_tool_artifact_counter(&default_artifact_dir(&workspace)),
            active_batch_llm_request_id: None,
            queued_artifact_index: HashMap::new(),
            act_batch_tracker: HashMap::new(),
            action_semantic_intents: HashMap::new(),
            last_act_reconcile: None,
            destructive_cmd_policy: DestructiveCmdPolicy::from_env(),
            file_write_tracker: FileWriteTracker::default(),
            write_paths_by_action: HashMap::new(),
            dirty_tracker: WorkspaceDirtyTracker::default(),
            context_merger: ContextMerger::default(),
            pending_decompose_request_id: None,
            agent_id: None,
            last_act_span_id: None,
            last_acted: None,
            last_verified: None,
            last_verifier_outcome: None,
            last_verifier_retry_policy: None,
            last_verifier_reward_bias: None,
            last_verifier_actionable_failure: None,
            errors_before: 0,
            stagnant_ticks: 0,
            last_action_kind: String::new(),
            halted: false,
            last_halt_reason: None,
            last_control_kind: None,
            last_control_event_id: None,
            pending_required_successor: None,
            last_reward_trace_id: None,
            last_reward_execution_id: None,
            last_reward_verify_span_id: None,
            goodness: None,
            delta_g: None,
            last_observed_error_count: 0,
            last_observed_goal_hash: 0,
            last_observed_facts_hash: 0,
            current_tick: 0,
            forced_primary_objective: None,
            forced_primary_strategy: None,
        }
    }

    pub fn semantic_observed_hash(&self, observed: &LoopObserved, semantic_summary: &SemanticStateSummary) -> u64 {
        let mut h = DefaultHasher::new();
        observed.error_count.hash(&mut h);
        observed.goal_text.hash(&mut h);
        semantic_summary.hash(&mut h);
        h.finish()
    }

    pub fn harness_repair_state(&self) -> HarnessRepairState {
        HarnessRepairState::from_loop_context(self)
    }

    pub fn prime_harness_repair_target(&mut self, target: &HarnessRepairTarget, failure_output: &str) {
        self.goal_text = Some(match (&target.crate_name, &target.failing_test) {
            (Some(crate_name), Some(test_name)) => {
                format!("repair harness failure in crate `{crate_name}` for test `{test_name}`")
            }
            (Some(crate_name), None) => format!("repair harness failure in crate `{crate_name}`"),
            (None, Some(test_name)) => format!("repair harness failure for test `{test_name}`"),
            (None, None) => "repair harness failure".to_string(),
        });
        let (failure_class, failure_scope) = crate::compiler_hints::classify_failure_metadata(failure_output);
        if self.last_observed.is_none() {
            self.last_observed = Some(canon_event::LoopObserved {
                tick: self.current_tick,
                error_count: 0,
                warning_count: 0,
                compiler_errors: Vec::new(),
                goal_text: self.goal_text.clone(),
                semantic_summary: canon_semantic_state::SemanticStateSummary::default(),
                observe_diagnostics: Vec::new(),
            });
        }
        if let Some(observed) = self.last_observed.as_mut() {
            observed.semantic_summary.complete = true;
            observed.semantic_summary.failure_class = Some(failure_class.as_str().to_string());
            observed.semantic_summary.failure_scope = Some(failure_scope.as_str().to_string());
            // CRITICAL FIX: properly emit LoopObserved with parent propagation
            let event = canon_event::RuntimeEvent::LoopObserved(observed.clone());

            // Derive parent_ids from last known control event (required invariant)
            let parent_ids = if let Some(parent) = &self.last_control_event_id {
                vec![canon_event::EventId::new(parent.clone())]
            } else {
                // Fail fast if no parent is available (do NOT silently emit invalid event)
                panic!("FATAL: missing parent_ids for LoopObserved emission; last_control_event_id is None");
            };

            // Emit with proper parent linkage
            let _ = self.emitter.emit_with_parents(event, parent_ids, file!(), line!());
        }
        self.forced_primary_objective = Some(DevelopmentObjectiveKind::ReduceCompilerFailures);
        self.forced_primary_strategy = Some(DevelopmentStrategyKind::SimplifyPlanBatch);
    }
}

fn default_artifact_dir(_workspace: &PathBuf) -> PathBuf {
    let default_dir = "/workspace/ai_sandbox/canon/canon-utils/state/reports_out/llm";
    std::env::var("CANON_LLM_LOG_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(default_dir))
}

fn next_tool_artifact_counter(log_dir: &std::path::Path) -> u32 {
    let mut max_seen: Option<u32> = None;
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some((n, _suffix, _ts)) = parse_artifact_name(&name) else {
            continue;
        };
        max_seen = Some(max_seen.map_or(n, |m| m.max(n)));
    }
    max_seen.map_or(0, |m| m.saturating_add(1))
}

fn parse_artifact_name(name: &str) -> Option<(u32, String, Option<u64>)> {
    let mut parts = name.splitn(3, '_');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next()?;

    if let Ok(n) = first.parse::<u32>() {
        return Some((n, format!("{}_{}", second, third), None));
    }
    let ts = first.parse::<u64>().ok()?;
    let n = second.parse::<u32>().ok()?;
    Some((n, third.to_string(), Some(ts)))
}

// ----------------- batch tracking helpers (ported) -----------------
impl LoopContext {
    pub fn artifact_index_for_plan(&mut self, planned: &LoopPlanned) -> u32 {
        if let Some(request_id) = planned.llm_request_id.as_deref() {
            if let Some(n) = find_request_index_by_request_id(&self.artifact_dir, request_id) {
                if let Some(cache_key) = plan_cache_key(planned) {
                    self.queued_artifact_index.insert(cache_key, n);
                }
                return n;
            }
        }
        if let Some(cache_key) = plan_cache_key(planned) {
            if let Some(n) = self.queued_artifact_index.get(&cache_key) {
                return *n;
            }
            let n = self.next_tool_artifact_n();
            self.queued_artifact_index.insert(cache_key, n);
            return n;
        }
        self.next_tool_artifact_n()
    }

    pub fn clear_cached_artifact_index_for_plan(&mut self, planned: &LoopPlanned) {
        if let Some(cache_key) = plan_cache_key(planned) {
            self.queued_artifact_index.remove(&cache_key);
        }
    }

    pub fn mark_batch_planned(&mut self, planned: &LoopPlanned, artifact_n: u32) {
        let Some(llm_request_id) = planned.llm_request_id.clone() else {
            return;
        };
        let snapshot = {
            let status = self.act_batch_tracker.entry(llm_request_id.clone()).or_insert_with(|| BatchStatus { artifact_n, ..BatchStatus::default() });
            if status.artifact_n == 0 {
                status.artifact_n = artifact_n;
            }
            status.planned = status.planned.saturating_add(1);
            status.clone()
        };
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, &llm_request_id, "in_progress", &snapshot);
    }

    pub fn mark_batch_dispatched(&mut self, planned: &LoopPlanned) {
        let Some(llm_request_id) = planned.llm_request_id.as_deref() else {
            return;
        };
        let Some(status) = self.act_batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        status.dispatched = status.dispatched.saturating_add(1);
        let snapshot = status.clone();
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, llm_request_id, "in_progress", &snapshot);
    }

    pub fn mark_batch_inline_completion(&mut self, planned: &LoopPlanned, success: bool) {
        self.mark_batch_completion(planned.llm_request_id.as_deref(), success);
    }

    pub fn mark_batch_completion(&mut self, llm_request_id: Option<&str>, success: bool) {
        let Some(llm_request_id) = llm_request_id else {
            return;
        };
        let mut should_remove = false;
        let Some(status) = self.act_batch_tracker.get_mut(llm_request_id) else {
            return;
        };
        if success {
            status.completed_ok = status.completed_ok.saturating_add(1);
        } else {
            status.completed_fail = status.completed_fail.saturating_add(1);
        }
        let finished = status.completed_ok.saturating_add(status.completed_fail) >= status.planned;
        let status_label = if finished {
            if status.completed_fail == 0 {
                "completed"
            } else {
                "failed_partial"
            }
        } else {
            "in_progress"
        };
        let snapshot = status.clone();
        write_batch_status_artifact(&self.artifact_dir, snapshot.artifact_n, llm_request_id, status_label, &snapshot);
        if finished {
            should_remove = true;
        }
        if should_remove {
            self.act_batch_tracker.remove(llm_request_id);
        }
    }

    fn next_tool_artifact_n(&mut self) -> u32 {
        let n = self.artifact_counter;
        self.artifact_counter = self.artifact_counter.saturating_add(1);
        n
    }
}

fn plan_cache_key(planned: &LoopPlanned) -> Option<String> {
    planned.action_id.as_ref().map(|id| format!("action:{id}")).or_else(|| planned.plan_step_id.as_ref().map(|id| format!("step:{id}")))
}

fn find_request_index_by_request_id(log_dir: &std::path::Path, request_id: &str) -> Option<u32> {
    let entries = std::fs::read_dir(log_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_str()?;
        let Some((n, suffix, _ts)) = parse_artifact_name(name) else {
            continue;
        };
        if suffix != "request.json" {
            continue;
        }
        let raw = std::fs::read_to_string(entry.path()).ok()?;
        let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
        if v.get("request_id").and_then(|x| x.as_str()) == Some(request_id) {
            return Some(n);
        }
    }
    None
}

fn write_batch_status_artifact(log_dir: &std::path::Path, artifact_n: u32, llm_request_id: &str, status: &str, batch: &BatchStatus) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = artifact_path_for(log_dir, artifact_n, "batch_status");
    let value = serde_json::json!({
        "n": artifact_n,
        "llm_request_id": llm_request_id,
        "status": status,
        "planned": batch.planned,
        "dispatched": batch.dispatched,
        "completed_ok": batch.completed_ok,
        "completed_fail": batch.completed_fail,
        "updated_ms": now_ms_u64(),
    });
    let serialized = serde_json::to_string_pretty(&value).unwrap_or_default();
    write_atomic(&path, &serialized);
}

// utility share with act stage
fn now_ms_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn artifact_path_for(log_dir: &std::path::Path, artifact_n: u32, suffix: &str) -> std::path::PathBuf {
    log_dir.join(format!("{:09}_{}.json", artifact_n, suffix))
}

fn write_atomic(path: &std::path::Path, content: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, content).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}
