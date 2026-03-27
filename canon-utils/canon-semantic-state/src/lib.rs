use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompilerHintKind {
    MissingModule,
    DeadCodeForbidConflict,
    MissingEntrypoint,
    UnresolvedImport,
    MissingSymbol,
    DuplicateDefinition,
    TraitBoundFailure,
    GenericCompilerFailure,
}

impl CompilerHintKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MissingModule => "missing_module",
            Self::DeadCodeForbidConflict => "dead_code_forbid_conflict",
            Self::MissingEntrypoint => "missing_entrypoint",
            Self::UnresolvedImport => "unresolved_import",
            Self::MissingSymbol => "missing_symbol",
            Self::DuplicateDefinition => "duplicate_definition",
            Self::TraitBoundFailure => "trait_bound_failure",
            Self::GenericCompilerFailure => "generic_compiler_failure",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "missing_module" => Some(Self::MissingModule),
            "dead_code_forbid_conflict" => Some(Self::DeadCodeForbidConflict),
            "missing_entrypoint" => Some(Self::MissingEntrypoint),
            "unresolved_import" => Some(Self::UnresolvedImport),
            "missing_symbol" => Some(Self::MissingSymbol),
            "duplicate_definition" => Some(Self::DuplicateDefinition),
            "trait_bound_failure" => Some(Self::TraitBoundFailure),
            "generic_compiler_failure" => Some(Self::GenericCompilerFailure),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerHintRecord {
    pub kind: String,
    pub summary: String,
    pub suggested_repair: String,
    pub target_files: Vec<String>,
}

impl CompilerHintRecord {
    pub fn new(
        kind: CompilerHintKind,
        summary: impl Into<String>,
        suggested_repair: impl Into<String>,
        target_files: Vec<String>,
    ) -> Self {
        Self {
            kind: kind.as_str().to_string(),
            summary: summary.into(),
            suggested_repair: suggested_repair.into(),
            target_files,
        }
    }

    pub fn kind_enum(&self) -> Option<CompilerHintKind> {
        CompilerHintKind::from_str(&self.kind)
    }

    pub fn render_line(&self) -> String {
        let targets = if self.target_files.is_empty() {
            "none".to_string()
        } else {
            self.target_files.join("|")
        };
        format!(
            "kind={} targets={} summary={} repair={}",
            self.kind, targets, self.summary, self.suggested_repair
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticStateSummary {
    pub version: u32,
    pub complete: bool,
    pub target_root: Option<String>,
    pub path_exists: bool,
    pub repo_initialized: bool,
    pub cargo_project: bool,
    pub crate_name: Option<String>,
    pub entrypoint_kind: Option<String>,
    pub rust_file_count: Option<usize>,
    pub source_files: Vec<String>,
    pub module_gaps: Vec<String>,
    pub planning_preconditions: Vec<String>,
    pub repair_intents: Vec<String>,
    pub compiler_hints: Vec<CompilerHintRecord>,
    pub validation_blocked_by_preconditions: bool,
    pub compiler_repair_required: bool,
    pub graph_artifact_id: Option<String>,
    pub graph_node_count: Option<usize>,
    pub graph_edge_count: Option<usize>,
    pub graph_file_count: Option<usize>,
    pub graph_call_edge_count: Option<usize>,
    pub graph_module_edge_count: Option<usize>,
    pub graph_cfg_edge_count: Option<usize>,
}

impl SemanticStateSummary {
    pub const VERSION: u32 = 1;

    pub fn to_workspace_facts(&self) -> Vec<String> {
        let mut facts = Vec::new();
        facts.push(format!("semantic.version={}", self.version));
        facts.push(format!("semantic.complete={}", self.complete));
        if let Some(target_root) = &self.target_root {
            facts.push(format!("semantic.target_root={target_root}"));
        }
        facts.push(format!("semantic.path_exists={}", self.path_exists));
        facts.push(format!("semantic.repo_initialized={}", self.repo_initialized));
        facts.push(format!("semantic.cargo_project={}", self.cargo_project));
        if let Some(crate_name) = &self.crate_name {
            facts.push(format!("semantic.crate_name={crate_name}"));
        }
        if let Some(entrypoint_kind) = &self.entrypoint_kind {
            facts.push(format!("semantic.entrypoint_kind={entrypoint_kind}"));
        }
        if let Some(rust_file_count) = self.rust_file_count {
            facts.push(format!("semantic.rust_file_count={rust_file_count}"));
        }
        for file in &self.source_files {
            facts.push(format!("semantic.source_file={file}"));
        }
        for gap in &self.module_gaps {
            facts.push(format!("semantic.module_gap={gap}"));
        }
        for precondition in &self.planning_preconditions {
            facts.push(format!("semantic.planning_precondition={precondition}"));
        }
        for intent in &self.repair_intents {
            facts.push(format!("semantic.repair_intent={intent}"));
        }
        for hint in &self.compiler_hints {
            facts.push(format!("semantic.compiler_hint={}", hint.render_line()));
        }
        facts.push(format!(
            "semantic.validation_blocked_by_preconditions={}",
            self.validation_blocked_by_preconditions
        ));
        facts.push(format!(
            "semantic.compiler_repair_required={}",
            self.compiler_repair_required
        ));
        if let Some(value) = &self.graph_artifact_id {
            facts.push(format!("semantic.graph_artifact_id={value}"));
        }
        if let Some(value) = self.graph_node_count {
            facts.push(format!("semantic.graph_node_count={value}"));
        }
        if let Some(value) = self.graph_edge_count {
            facts.push(format!("semantic.graph_edge_count={value}"));
        }
        if let Some(value) = self.graph_file_count {
            facts.push(format!("semantic.graph_file_count={value}"));
        }
        if let Some(value) = self.graph_call_edge_count {
            facts.push(format!("semantic.graph_call_edge_count={value}"));
        }
        if let Some(value) = self.graph_module_edge_count {
            facts.push(format!("semantic.graph_module_edge_count={value}"));
        }
        if let Some(value) = self.graph_cfg_edge_count {
            facts.push(format!("semantic.graph_cfg_edge_count={value}"));
        }
        facts
    }

    pub fn apply_graph_artifact_summary(
        &mut self,
        artifact_id: String,
        node_count: usize,
        edge_count: usize,
        file_count: usize,
        call_edge_count: usize,
        module_edge_count: usize,
        cfg_edge_count: usize,
    ) {
        self.graph_artifact_id = Some(artifact_id);
        self.graph_node_count = Some(node_count);
        self.graph_edge_count = Some(edge_count);
        self.graph_file_count = Some(file_count);
        self.graph_call_edge_count = Some(call_edge_count);
        self.graph_module_edge_count = Some(module_edge_count);
        self.graph_cfg_edge_count = Some(cfg_edge_count);
    }

    pub fn apply_rustc_capture_failure(&mut self, message: &str) {
        self.compiler_repair_required = true;
        if !self.compiler_hints.iter().any(|hint| {
            hint.kind_enum() == Some(CompilerHintKind::GenericCompilerFailure)
                && hint.summary == format!("rustc capture failed: {message}")
        }) {
            self.compiler_hints.push(CompilerHintRecord::new(
                CompilerHintKind::GenericCompilerFailure,
                format!("rustc capture failed: {message}"),
                "refresh compiler context or stabilize rustc capture before structural analysis",
                Vec::new(),
            ));
        }
    }

    pub fn from_workspace_facts(facts: &[String]) -> Self {
        let mut summary = Self { version: Self::VERSION, ..Self::default() };
        for fact in facts {
            if let Some(value) = fact.strip_prefix("semantic.version=") {
                summary.version = value.parse::<u32>().unwrap_or(Self::VERSION);
            } else if let Some(value) = fact.strip_prefix("semantic.complete=") {
                summary.complete = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.target_root=") {
                summary.target_root = Some(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.path_exists=") {
                summary.path_exists = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.repo_initialized=") {
                summary.repo_initialized = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.cargo_project=") {
                summary.cargo_project = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.crate_name=") {
                summary.crate_name = Some(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.entrypoint_kind=") {
                summary.entrypoint_kind = Some(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.rust_file_count=") {
                summary.rust_file_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.source_file=") {
                summary.source_files.push(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.module_gap=") {
                summary.module_gaps.push(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.planning_precondition=") {
                summary.planning_preconditions.push(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.repair_intent=") {
                summary.repair_intents.push(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.compiler_hint=") {
                if let Some(hint) = parse_compiler_hint_record(value) {
                    summary.compiler_hints.push(hint);
                }
            } else if let Some(value) =
                fact.strip_prefix("semantic.validation_blocked_by_preconditions=")
            {
                summary.validation_blocked_by_preconditions = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.compiler_repair_required=") {
                summary.compiler_repair_required = value == "true";
            } else if let Some(value) = fact.strip_prefix("semantic.graph_artifact_id=") {
                summary.graph_artifact_id = Some(value.to_string());
            } else if let Some(value) = fact.strip_prefix("semantic.graph_node_count=") {
                summary.graph_node_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.graph_edge_count=") {
                summary.graph_edge_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.graph_file_count=") {
                summary.graph_file_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.graph_call_edge_count=") {
                summary.graph_call_edge_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.graph_module_edge_count=") {
                summary.graph_module_edge_count = value.parse::<usize>().ok();
            } else if let Some(value) = fact.strip_prefix("semantic.graph_cfg_edge_count=") {
                summary.graph_cfg_edge_count = value.parse::<usize>().ok();
            }
        }
        summary
    }

    pub fn planner_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("semantic_version={}", self.version));
        lines.push(format!("semantic_complete={}", self.complete));
        if let Some(target_root) = &self.target_root {
            lines.push(format!("target_root={target_root}"));
        }
        lines.push(format!("path_exists={}", self.path_exists));
        lines.push(format!("repo_initialized={}", self.repo_initialized));
        lines.push(format!("cargo_project={}", self.cargo_project));
        if let Some(crate_name) = &self.crate_name {
            lines.push(format!("crate_name={crate_name}"));
        }
        if let Some(entrypoint_kind) = &self.entrypoint_kind {
            lines.push(format!("entrypoint_kind={entrypoint_kind}"));
        }
        if let Some(rust_file_count) = self.rust_file_count {
            lines.push(format!("rust_file_count={rust_file_count}"));
        }
        if !self.source_files.is_empty() {
            lines.push(format!("file_graph={}", self.source_files.join(", ")));
        }
        if let Some(graph_artifact_id) = &self.graph_artifact_id {
            lines.push(format!("graph_artifact_id={graph_artifact_id}"));
        }
        if let Some(graph_node_count) = self.graph_node_count {
            lines.push(format!("graph_node_count={graph_node_count}"));
        }
        if let Some(graph_edge_count) = self.graph_edge_count {
            lines.push(format!("graph_edge_count={graph_edge_count}"));
        }
        lines
    }

    pub fn compiler_hint_kinds(&self) -> Vec<&str> {
        self.compiler_hints.iter().map(|hint| hint.kind.as_str()).collect()
    }

    pub fn has_actionable_compiler_hints(&self) -> bool {
        self.compiler_hints.iter().any(|hint| {
            hint.kind_enum().is_some_and(|kind| {
                matches!(
                    kind,
                    CompilerHintKind::MissingModule
                        | CompilerHintKind::DeadCodeForbidConflict
                        | CompilerHintKind::MissingEntrypoint
                        | CompilerHintKind::UnresolvedImport
                        | CompilerHintKind::MissingSymbol
                        | CompilerHintKind::DuplicateDefinition
                        | CompilerHintKind::TraitBoundFailure
                )
            })
        })
    }

    pub fn compact_block(&self) -> String {
        let mut parts = vec![
            format!("version={}", self.version),
            format!("complete={}", self.complete),
            format!("path_exists={}", self.path_exists),
            format!("cargo_project={}", self.cargo_project),
            format!(
                "entrypoint_kind={}",
                self.entrypoint_kind.as_deref().unwrap_or("NA")
            ),
            format!("crate_name={}", self.crate_name.as_deref().unwrap_or("NA")),
            format!(
                "validation_blocked={}",
                self.validation_blocked_by_preconditions
            ),
            format!("compiler_repair_required={}", self.compiler_repair_required),
        ];
        if !self.planning_preconditions.is_empty() {
            parts.push(format!("preconditions={}", self.planning_preconditions.join("|")));
        }
        if !self.repair_intents.is_empty() {
            parts.push(format!("repair_intents={}", self.repair_intents.join("|")));
        }
        if !self.compiler_hints.is_empty() {
            parts.push(format!("compiler_hint_kinds={}", self.compiler_hint_kinds().join("|")));
        }
        if let Some(graph_artifact_id) = &self.graph_artifact_id {
            parts.push(format!("graph_artifact_id={graph_artifact_id}"));
        }
        if let Some(graph_node_count) = self.graph_node_count {
            parts.push(format!("graph_nodes={graph_node_count}"));
        }
        if let Some(graph_edge_count) = self.graph_edge_count {
            parts.push(format!("graph_edges={graph_edge_count}"));
        }
        parts.join("\n")
    }

    pub fn render_planner_block(&self) -> String {
        let compiler_hint_lines = self
            .compiler_hints
            .iter()
            .map(CompilerHintRecord::render_line)
            .collect::<Vec<_>>();
        format!(
            "Environment model:\n{}\n\nPlanning preconditions:\n{}\n\nRepair intents:\n{}\n\nCompiler hints:\n{}\n\nSemantic summary:\n{}",
            render_bullets(&self.planner_lines()),
            render_bullets(&self.planning_preconditions),
            render_bullets(&self.repair_intents),
            render_bullets(&compiler_hint_lines),
            self.compact_block(),
        )
    }

    pub fn render_route_block(&self) -> String {
        format!("Semantic summary:\n{}", self.compact_block())
    }
}


#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LlmSemanticContext {
    pub mission_summary: Option<String>,
    pub semantic_summary: SemanticStateSummary,
    pub objective_state: SelfDevelopmentObjectiveState,
    pub objective_trend_state: ObjectiveTrendState,
    pub target_workspace: Option<String>,
    pub workspace_loc: Option<usize>,
    pub error_count: Option<usize>,
    pub warning_count: Option<usize>,
    pub route_rationale: Option<String>,
    pub route_confidence: Option<f64>,
    pub invalid_plan_reason: Option<String>,
    pub invalid_plan_planned_count: Option<usize>,
    pub consecutive_invalid_plan_batches: u32,
    pub low_level_diagnostics: Vec<String>,
    pub recent_actions: Vec<String>,
    pub recent_tool_results: Vec<String>,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SelfDevelopmentObjectiveState {
    pub semantic_progress_rate: f32,
    pub semantic_no_progress_streak: usize,
    pub consecutive_invalid_plan_batches: u32,
    pub validation_blocked_by_preconditions: bool,
    pub compiler_repair_required: bool,
    pub misalignment_pressure_score: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DevelopmentObjectiveKind {
    ReduceCompilerFailures,
    ReduceContradictionRate,
    IncreaseTestCoverage,
    DecreaseInvalidPlanRate,
    ReduceStalledLoopFrequency,
    ImproveModuleCohesion,
}

impl DevelopmentObjectiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReduceCompilerFailures => "reduce_compiler_failures",
            Self::ReduceContradictionRate => "reduce_contradiction_rate",
            Self::IncreaseTestCoverage => "increase_test_coverage",
            Self::DecreaseInvalidPlanRate => "decrease_invalid_plan_rate",
            Self::ReduceStalledLoopFrequency => "reduce_stalled_loop_frequency",
            Self::ImproveModuleCohesion => "improve_module_cohesion",
        }
    }

    pub fn focus_text(self) -> &'static str {
        match self {
            Self::ReduceCompilerFailures => "reduce compiler failures",
            Self::ReduceContradictionRate => "reduce contradiction rate",
            Self::IncreaseTestCoverage => "increase test coverage",
            Self::DecreaseInvalidPlanRate => "decrease invalid-plan rate",
            Self::ReduceStalledLoopFrequency => "reduce stalled-loop frequency",
            Self::ImproveModuleCohesion => "improve module cohesion",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentObjective {
    pub kind: DevelopmentObjectiveKind,
    pub priority_score: u32,
    pub rationale: String,
    pub progress_summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DevelopmentStrategyKind {
    FixConfigLintPolicy,
    ApplyTargetedCompilerRepair,
    PlanSymbolAwareRename,
    DiscoverTestSurface,
    AddRegressionTest,
    SimplifyPlanBatch,
    RealignObjectiveFlow,
    RefreshContextBeforeRetry,
    CreateMissingModules,
    RestructureModules,
}

impl DevelopmentStrategyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FixConfigLintPolicy => "fix_config_lint_policy",
            Self::ApplyTargetedCompilerRepair => "apply_targeted_compiler_repair",
            Self::PlanSymbolAwareRename => "plan_symbol_aware_rename",
            Self::DiscoverTestSurface => "discover_test_surface",
            Self::AddRegressionTest => "add_regression_test",
            Self::SimplifyPlanBatch => "simplify_plan_batch",
            Self::RealignObjectiveFlow => "realign_objective_flow",
            Self::RefreshContextBeforeRetry => "refresh_context_before_retry",
            Self::CreateMissingModules => "create_missing_modules",
            Self::RestructureModules => "restructure_modules",
        }
    }

    pub fn focus_text(self) -> &'static str {
        match self {
            Self::FixConfigLintPolicy => "edit workspace/toolchain config before changing source",
            Self::ApplyTargetedCompilerRepair => "make the smallest source repair that directly addresses the compiler blocker",
            Self::PlanSymbolAwareRename => "use graph-backed symbol relationships to plan a safe rename before editing",
            Self::DiscoverTestSurface => "inspect the current test surface before adding tests",
            Self::AddRegressionTest => "add a targeted regression test for the active behavior gap",
            Self::SimplifyPlanBatch => "reduce batch complexity and constrain the next plan",
            Self::RealignObjectiveFlow => "realign goal, route, and planner around the same objective",
            Self::RefreshContextBeforeRetry => "refresh semantic context before repeating a stalled repair",
            Self::CreateMissingModules => "create or wire missing module files before validation",
            Self::RestructureModules => "improve module boundaries and cohesion before more feature work",
        }
    }
}

impl SelfDevelopmentObjectiveState {
    pub fn is_stalled(&self) -> bool {
        self.semantic_no_progress_streak >= 2 || self.consecutive_invalid_plan_batches >= 2
    }

    pub fn repair_pressure_score(&self) -> u32 {
        let mut score = self.consecutive_invalid_plan_batches + self.misalignment_pressure_score;
        if self.validation_blocked_by_preconditions {
            score += 1;
        }
        if self.compiler_repair_required {
            score += 1;
        }
        if self.semantic_no_progress_streak > 0 {
            score += 1;
        }
        score
    }

    pub fn render_lines(&self) -> Vec<String> {
        vec![
            format!("semantic_progress_rate={:.2}", self.semantic_progress_rate),
            format!("semantic_no_progress_streak={}", self.semantic_no_progress_streak),
            format!(
                "consecutive_invalid_plan_batches={}",
                self.consecutive_invalid_plan_batches
            ),
            format!(
                "validation_blocked_by_preconditions={}",
                self.validation_blocked_by_preconditions
            ),
            format!("compiler_repair_required={}", self.compiler_repair_required),
            format!("misalignment_pressure_score={}", self.misalignment_pressure_score),
            format!("repair_pressure_score={}", self.repair_pressure_score()),
            format!("repair_loop_stalled={}", self.is_stalled()),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveTrendState {
    pub planning_attempts: u32,
    pub invalid_plan_events: u32,
    pub total_execution_results: u32,
    pub semantic_progress_events: u32,
    pub no_semantic_progress_events: u32,
    pub current_no_progress_streak: u32,
    pub repeated_stall_count: u32,
    pub route_objective_contradiction_events: u32,
    pub goal_objective_drift_events: u32,
    pub baseline_error_count: Option<u32>,
    pub current_error_count: Option<u32>,
    pub baseline_module_gap_count: Option<u32>,
    pub current_module_gap_count: Option<u32>,
    pub baseline_test_surface_count: Option<u32>,
    pub current_test_surface_count: Option<u32>,
    pub last_goodness: Option<f32>,
    pub last_delta_g: Option<f32>,
}

impl ObjectiveTrendState {
    pub fn record_execution_results(&mut self, results: &[SemanticExecutionResultRecord]) {
        for result in results {
            self.total_execution_results = self.total_execution_results.saturating_add(1);
            if result.semantic_progress {
                self.semantic_progress_events = self.semantic_progress_events.saturating_add(1);
                self.current_no_progress_streak = 0;
            } else {
                self.no_semantic_progress_events = self.no_semantic_progress_events.saturating_add(1);
                self.current_no_progress_streak = self.current_no_progress_streak.saturating_add(1);
                if self.current_no_progress_streak == 2 {
                    self.repeated_stall_count = self.repeated_stall_count.saturating_add(1);
                }
            }
        }
    }

    pub fn record_planning_completion(&mut self, status: &str) {
        self.planning_attempts = self.planning_attempts.saturating_add(1);
        if status == "invalid_plan" {
            self.invalid_plan_events = self.invalid_plan_events.saturating_add(1);
        }
    }

    pub fn record_invalid_plan_event(&mut self) {
        self.invalid_plan_events = self.invalid_plan_events.saturating_add(1);
    }

    pub fn record_goodness(&mut self, g: f32, delta_g: f32) {
        self.last_goodness = Some(g);
        self.last_delta_g = Some(delta_g);
    }

    pub fn record_observation(&mut self, error_count: usize, semantic_summary: &SemanticStateSummary) {
        let error_count = error_count as u32;
        self.current_error_count = Some(error_count);
        self.baseline_error_count.get_or_insert(error_count);

        let module_gap_count = semantic_summary.module_gaps.len() as u32;
        self.current_module_gap_count = Some(module_gap_count);
        self.baseline_module_gap_count.get_or_insert(module_gap_count);

        let test_surface_count = semantic_summary
            .source_files
            .iter()
            .filter(|path| {
                path.contains("/tests/")
                    || path.starts_with("tests/")
                    || path.ends_with("_test.rs")
                    || path.ends_with("_tests.rs")
            })
            .count() as u32;
        self.current_test_surface_count = Some(test_surface_count);
        self.baseline_test_surface_count.get_or_insert(test_surface_count);
    }

    pub fn record_route_objective_contradiction(&mut self) {
        self.route_objective_contradiction_events =
            self.route_objective_contradiction_events.saturating_add(1);
    }

    pub fn record_goal_objective_drift(&mut self) {
        self.goal_objective_drift_events = self.goal_objective_drift_events.saturating_add(1);
    }

    pub fn repair_resolution_rate(&self) -> f32 {
        if self.total_execution_results == 0 {
            0.0
        } else {
            self.semantic_progress_events as f32 / self.total_execution_results as f32
        }
    }

    pub fn invalid_plan_rate(&self) -> f32 {
        if self.planning_attempts == 0 {
            0.0
        } else {
            self.invalid_plan_events as f32 / self.planning_attempts as f32
        }
    }

    pub fn semantic_progress_trend(&self) -> f32 {
        self.repair_resolution_rate() - (self.invalid_plan_rate() * 0.5)
    }

    pub fn misalignment_pressure_score(&self) -> u32 {
        self.route_objective_contradiction_events + self.goal_objective_drift_events
    }

    pub fn compiler_error_delta(&self) -> i32 {
        self.current_error_count.unwrap_or(0) as i32 - self.baseline_error_count.unwrap_or(0) as i32
    }

    pub fn module_gap_delta(&self) -> i32 {
        self.current_module_gap_count.unwrap_or(0) as i32
            - self.baseline_module_gap_count.unwrap_or(0) as i32
    }

    pub fn test_surface_delta(&self) -> i32 {
        self.current_test_surface_count.unwrap_or(0) as i32
            - self.baseline_test_surface_count.unwrap_or(0) as i32
    }

    pub fn render_lines(&self) -> Vec<String> {
        vec![
            format!("repair_resolution_rate={:.2}", self.repair_resolution_rate()),
            format!("invalid_plan_rate={:.2}", self.invalid_plan_rate()),
            format!("semantic_progress_trend={:.2}", self.semantic_progress_trend()),
            format!("repeated_stall_count={}", self.repeated_stall_count),
            format!(
                "route_objective_contradiction_events={}",
                self.route_objective_contradiction_events
            ),
            format!(
                "goal_objective_drift_events={}",
                self.goal_objective_drift_events
            ),
            format!("compiler_error_delta={}", self.compiler_error_delta()),
            format!("module_gap_delta={}", self.module_gap_delta()),
            format!("test_surface_delta={}", self.test_surface_delta()),
            format!("total_execution_results={}", self.total_execution_results),
            format!("planning_attempts={}", self.planning_attempts),
            format!(
                "last_goodness={}",
                self.last_goodness
                    .map(|v| format!("{v:.3}"))
                    .unwrap_or_else(|| "NA".into())
            ),
            format!(
                "last_delta_g={}",
                self.last_delta_g
                    .map(|v| format!("{v:.3}"))
                    .unwrap_or_else(|| "NA".into())
            ),
        ]
    }

    pub fn primary_objective(&self, objective_state: &SelfDevelopmentObjectiveState) -> &'static str {
        primary_development_objective_kind(objective_state, self, &SemanticStateSummary::default())
            .focus_text()
    }
}

pub fn primary_development_objective_kind(
    objective_state: &SelfDevelopmentObjectiveState,
    objective_trend_state: &ObjectiveTrendState,
    semantic_summary: &SemanticStateSummary,
) -> DevelopmentObjectiveKind {
    derive_development_objectives(semantic_summary, objective_state, objective_trend_state)
        .into_iter()
        .next()
        .map(|objective| objective.kind)
        .unwrap_or(DevelopmentObjectiveKind::ReduceCompilerFailures)
}

pub fn primary_development_strategy_kind(
    objective_state: &SelfDevelopmentObjectiveState,
    objective_trend_state: &ObjectiveTrendState,
    semantic_summary: &SemanticStateSummary,
) -> DevelopmentStrategyKind {
    let primary_objective =
        primary_development_objective_kind(objective_state, objective_trend_state, semantic_summary);

    if objective_state.misalignment_pressure_score > 0 {
        return DevelopmentStrategyKind::RealignObjectiveFlow;
    }
    if semantic_summary
        .compiler_hints
        .iter()
        .any(|hint| hint.kind_enum() == Some(CompilerHintKind::DeadCodeForbidConflict))
    {
        return DevelopmentStrategyKind::FixConfigLintPolicy;
    }
    if objective_trend_state.repeated_stall_count > 0
        && objective_state.semantic_no_progress_streak >= 2
    {
        return DevelopmentStrategyKind::RefreshContextBeforeRetry;
    }
    if semantic_summary.graph_artifact_id.is_some()
        && semantic_summary
            .compiler_hints
            .iter()
            .any(|hint| hint.kind_enum() == Some(CompilerHintKind::DuplicateDefinition))
    {
        return DevelopmentStrategyKind::PlanSymbolAwareRename;
    }

    match primary_objective {
        DevelopmentObjectiveKind::ReduceCompilerFailures => {
            if !semantic_summary.module_gaps.is_empty() {
                DevelopmentStrategyKind::CreateMissingModules
            } else {
                DevelopmentStrategyKind::ApplyTargetedCompilerRepair
            }
        }
        DevelopmentObjectiveKind::ReduceContradictionRate => DevelopmentStrategyKind::RealignObjectiveFlow,
        DevelopmentObjectiveKind::IncreaseTestCoverage => {
            let has_test_files = semantic_summary.source_files.iter().any(|path| {
                path.contains("/tests/")
                    || path.starts_with("tests/")
                    || path.ends_with("_test.rs")
                    || path.ends_with("_tests.rs")
            });
            if has_test_files {
                DevelopmentStrategyKind::AddRegressionTest
            } else {
                DevelopmentStrategyKind::DiscoverTestSurface
            }
        }
        DevelopmentObjectiveKind::DecreaseInvalidPlanRate => DevelopmentStrategyKind::SimplifyPlanBatch,
        DevelopmentObjectiveKind::ReduceStalledLoopFrequency => {
            DevelopmentStrategyKind::RefreshContextBeforeRetry
        }
        DevelopmentObjectiveKind::ImproveModuleCohesion => {
            let has_dense_module_graph = semantic_summary
                .graph_module_edge_count
                .zip(semantic_summary.graph_call_edge_count)
                .is_some_and(|(module_edges, call_edges)| {
                    module_edges > call_edges.saturating_mul(4) && module_edges > 32
                });
            if !semantic_summary.module_gaps.is_empty() {
                DevelopmentStrategyKind::CreateMissingModules
            } else if has_dense_module_graph && semantic_summary.graph_artifact_id.is_some() {
                DevelopmentStrategyKind::RestructureModules
            } else {
                DevelopmentStrategyKind::RestructureModules
            }
        }
    }
}

pub fn derive_development_objectives(
    semantic_summary: &SemanticStateSummary,
    objective_state: &SelfDevelopmentObjectiveState,
    objective_trend_state: &ObjectiveTrendState,
) -> Vec<DevelopmentObjective> {
    let has_test_files = semantic_summary.source_files.iter().any(|path| {
        path.contains("/tests/")
            || path.starts_with("tests/")
            || path.ends_with("_test.rs")
            || path.ends_with("_tests.rs")
    });
    let rust_file_count = semantic_summary.rust_file_count.unwrap_or(0);
    let mut objectives = vec![
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::ReduceCompilerFailures,
            priority_score: u32::from(objective_state.compiler_repair_required)
                + u32::from(objective_state.validation_blocked_by_preconditions)
                + u32::from(!semantic_summary.compiler_hints.is_empty())
                + objective_trend_state.compiler_error_delta().max(0) as u32,
            rationale: "compiler repair pressure is still present".to_string(),
            progress_summary: format!(
                "compiler_repair_required={} validation_blocked={} compiler_error_delta={}",
                objective_state.compiler_repair_required,
                objective_state.validation_blocked_by_preconditions,
                objective_trend_state.compiler_error_delta()
            ),
        },
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::ReduceContradictionRate,
            priority_score: objective_state.misalignment_pressure_score,
            rationale: "goal/route/planner drift has been observed".to_string(),
            progress_summary: format!(
                "route_contradictions={} goal_drifts={}",
                objective_trend_state.route_objective_contradiction_events,
                objective_trend_state.goal_objective_drift_events
            ),
        },
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::IncreaseTestCoverage,
            priority_score: u32::from(semantic_summary.cargo_project && !has_test_files)
                + u32::from(semantic_summary.path_exists && semantic_summary.cargo_project)
                + u32::from(objective_trend_state.test_surface_delta() <= 0),
            rationale: "the workspace has little or no visible test surface".to_string(),
            progress_summary: format!(
                "has_test_files={has_test_files} rust_file_count={rust_file_count} test_surface_delta={}",
                objective_trend_state.test_surface_delta()
            ),
        },
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::DecreaseInvalidPlanRate,
            priority_score: if objective_trend_state.invalid_plan_rate() > 0.0 {
                1 + objective_trend_state.invalid_plan_events
            } else {
                0
            },
            rationale: "invalid plans are reducing execution throughput".to_string(),
            progress_summary: format!(
                "invalid_plan_events={} invalid_plan_rate={:.2}",
                objective_trend_state.invalid_plan_events,
                objective_trend_state.invalid_plan_rate()
            ),
        },
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::ReduceStalledLoopFrequency,
            priority_score: objective_trend_state.repeated_stall_count
                + objective_state.semantic_no_progress_streak as u32,
            rationale: "the loop is repeating without semantic progress".to_string(),
            progress_summary: format!(
                "no_progress_streak={} repeated_stall_count={}",
                objective_state.semantic_no_progress_streak,
                objective_trend_state.repeated_stall_count
            ),
        },
        DevelopmentObjective {
            kind: DevelopmentObjectiveKind::ImproveModuleCohesion,
            priority_score: semantic_summary.module_gaps.len() as u32
                + u32::from(rust_file_count >= 8)
                + u32::from(
                    semantic_summary
                        .graph_module_edge_count
                        .zip(semantic_summary.graph_call_edge_count)
                        .is_some_and(|(module_edges, call_edges)| module_edges > call_edges.saturating_mul(8)),
                )
                + objective_trend_state.module_gap_delta().max(0) as u32,
            rationale: "module gaps or graph sprawl indicate structural cohesion issues".to_string(),
            progress_summary: format!(
                "module_gaps={} rust_file_count={} module_gap_delta={} graph_module_edges={} graph_call_edges={}",
                semantic_summary.module_gaps.len(),
                rust_file_count,
                objective_trend_state.module_gap_delta(),
                semantic_summary.graph_module_edge_count.unwrap_or(0),
                semantic_summary.graph_call_edge_count.unwrap_or(0)
            ),
        },
    ];
    objectives.sort_by(|a, b| {
        b.priority_score
            .cmp(&a.priority_score)
            .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
    });
    objectives
}

fn development_objective_lines(
    semantic_summary: &SemanticStateSummary,
    objective_state: &SelfDevelopmentObjectiveState,
    objective_trend_state: &ObjectiveTrendState,
) -> Vec<String> {
    derive_development_objectives(semantic_summary, objective_state, objective_trend_state)
        .into_iter()
        .filter(|objective| objective.priority_score > 0)
        .map(|objective| {
            format!(
                "kind={} priority={} progress={} rationale={}",
                objective.kind.as_str(),
                objective.priority_score,
                objective.progress_summary,
                objective.rationale
            )
        })
        .collect()
}

impl LlmSemanticContext {
    pub fn render_goal_gen_block(&self) -> String {
        let mut lines = Vec::new();
        let primary_objective = primary_development_objective_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let primary_strategy = primary_development_strategy_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        if let Some(mission) = &self.mission_summary {
            lines.push(format!("mission_summary={mission}"));
        }
        lines.push(format!("semantic_complete={}", self.semantic_summary.complete));
        lines.push(format!("path_exists={}", self.semantic_summary.path_exists));
        lines.push(format!("cargo_project={}", self.semantic_summary.cargo_project));
        if let Some(entrypoint_kind) = &self.semantic_summary.entrypoint_kind {
            lines.push(format!("entrypoint_kind={entrypoint_kind}"));
        }
        if !self.semantic_summary.compiler_hints.is_empty() {
            lines.push(format!(
                "compiler_hint_kinds={}",
                self.semantic_summary.compiler_hint_kinds().join("|")
            ));
        }
        lines.push(format!(
            "primary_objective={}",
            primary_objective.as_str()
        ));
        lines.push(format!("primary_objective_focus={}", primary_objective.focus_text()));
        lines.push(format!("primary_strategy={}", primary_strategy.as_str()));
        lines.push(format!("primary_strategy_focus={}", primary_strategy.focus_text()));
        lines.extend(development_objective_lines(
            &self.semantic_summary,
            &self.objective_state,
            &self.objective_trend_state,
        ));
        lines.extend(self.objective_state.render_lines());
        lines.extend(self.objective_trend_state.render_lines());
        format!("LLM semantic context:
{}", render_bullets(&lines))
    }

    pub fn render_router_block(&self) -> String {
        let primary_objective = primary_development_objective_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let primary_strategy = primary_development_strategy_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let mut lines = vec![
            format!("semantic_complete={}", self.semantic_summary.complete),
            format!("validation_blocked={}", self.semantic_summary.validation_blocked_by_preconditions),
            format!("compiler_repair_required={}", self.semantic_summary.compiler_repair_required),
        ];
        if let Some(rationale) = &self.route_rationale {
            lines.push(format!("route_rationale={rationale}"));
        }
        if let Some(confidence) = self.route_confidence {
            lines.push(format!("route_confidence={confidence:.2}"));
        }
        lines.push(self.semantic_summary.compact_block());
        lines.push(format!(
            "primary_objective={}",
            primary_objective.as_str()
        ));
        lines.push(format!("primary_objective_focus={}", primary_objective.focus_text()));
        lines.push(format!("primary_strategy={}", primary_strategy.as_str()));
        lines.push(format!("primary_strategy_focus={}", primary_strategy.focus_text()));
        if !self.recent_execution_results.is_empty() {
            lines.push(format!(
                "execution_results={}",
                self.recent_execution_results
                    .iter()
                    .map(SemanticExecutionResultRecord::render_line)
                    .collect::<Vec<_>>()
                    .join("|")
            ));
        }
        lines.extend(development_objective_lines(
            &self.semantic_summary,
            &self.objective_state,
            &self.objective_trend_state,
        ));
        lines.extend(self.objective_state.render_lines());
        lines.extend(self.objective_trend_state.render_lines());
        format!("LLM semantic context:
{}", render_bullets(&lines))
    }

    pub fn render_planner_base_block(&self) -> String {
        let primary_objective = primary_development_objective_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let primary_strategy = primary_development_strategy_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let mut sections = vec![
            self.semantic_summary.render_planner_block(),
            format!(
                "Primary objective:\n- {} ({})",
                primary_objective.as_str(),
                primary_objective.focus_text()
            ),
            format!(
                "Primary strategy:\n- {} ({})",
                primary_strategy.as_str(),
                primary_strategy.focus_text()
            ),
            format!(
                "Development objectives:\n{}",
                render_bullets(&development_objective_lines(
                    &self.semantic_summary,
                    &self.objective_state,
                    &self.objective_trend_state,
                ))
            ),
        ];
        if !self.low_level_diagnostics.is_empty() {
            sections.push(format!(
                "Low-level diagnostics:
{}",
                render_bullets(&self.low_level_diagnostics)
            ));
        }
        if !self.recent_execution_results.is_empty() {
            sections.push(format!(
                "Execution semantics:
{}",
                render_bullets(
                    &self
                        .recent_execution_results
                        .iter()
                        .map(SemanticExecutionResultRecord::render_line)
                        .collect::<Vec<_>>()
                )
            ));
            sections.push(format!(
                "Execution metrics:\n{}",
                render_bullets(&self.objective_state.render_lines()),
            ));
        }
        sections.push(format!(
            "Execution trends:\n{}",
            render_bullets(&self.objective_trend_state.render_lines()),
        ));
        sections.join("

")
    }

    pub fn render_planner_delta_block(&self) -> String {
        let primary_objective = primary_development_objective_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let primary_strategy = primary_development_strategy_kind(
            &self.objective_state,
            &self.objective_trend_state,
            &self.semantic_summary,
        );
        let route_section = match &self.route_rationale {
            Some(rationale) if !rationale.is_empty() => {
                let conf = self
                    .route_confidence
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "n/a".to_string());
                format!("Route rationale: {rationale}
Route confidence: {conf}")
            }
            _ => "Route rationale: (not provided)".to_string(),
        };
        let invalid_plan_section = match &self.invalid_plan_reason {
            Some(reason) => format!(
                "Invalid plan memory: consecutive_invalid_plan_batches={count}; last_invalid_plan_planned_count={planned}; last_invalid_plan_reason={reason}",
                count = self.consecutive_invalid_plan_batches,
                planned = self
                    .invalid_plan_planned_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "NA".to_string()),
            ),
            None => "Invalid plan memory: none".to_string(),
        };
        let compiler_hints = self
            .semantic_summary
            .compiler_hints
            .iter()
            .map(CompilerHintRecord::render_line)
            .collect::<Vec<_>>();
        let mut sections = vec![
            format!(
                "TARGET WORKSPACE: {}
All relative paths resolve against TARGET WORKSPACE (not its parent).
LOC: {}  |  Errors: {}  |  Warnings: {}",
                self.target_workspace.as_deref().unwrap_or("NA"),
                self.workspace_loc
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "NA".to_string()),
                self.error_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "NA".to_string()),
                self.warning_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "NA".to_string()),
            ),
            route_section,
            invalid_plan_section,
            format!("Compiler repair hints:
{}", render_bullets(&compiler_hints)),
            format!("Semantic summary:
{}", self.semantic_summary.compact_block()),
            format!(
                "Primary objective:\n- {} ({})",
                primary_objective.as_str(),
                primary_objective.focus_text()
            ),
            format!(
                "Primary strategy:\n- {} ({})",
                primary_strategy.as_str(),
                primary_strategy.focus_text()
            ),
            format!(
                "Development objectives:\n{}",
                render_bullets(&development_objective_lines(
                    &self.semantic_summary,
                    &self.objective_state,
                    &self.objective_trend_state,
                ))
            ),
        ];
        if !self.recent_actions.is_empty() {
            sections.push(format!("Recent actions:
{}", self.recent_actions.join("
")));
        }
        if !self.recent_tool_results.is_empty() {
            sections.push(format!("Recent tool results:
{}", self.recent_tool_results.join("
")));
        }
        if !self.recent_execution_results.is_empty() {
            sections.push(format!(
                "Recent execution semantics:
{}",
                self.recent_execution_results
                    .iter()
                    .map(SemanticExecutionResultRecord::render_line)
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        sections.push(format!(
            "Self-development objective state:\n{}",
            render_bullets(&self.objective_state.render_lines())
        ));
        sections.push(format!(
            "Self-development objective trends:\n{}",
            render_bullets(&self.objective_trend_state.render_lines())
        ));
        sections.join("

")
    }
}

pub fn derive_self_development_objective_state(
    semantic_summary: &SemanticStateSummary,
    consecutive_invalid_plan_batches: u32,
    recent_execution_results: &[SemanticExecutionResultRecord],
    objective_trend_state: &ObjectiveTrendState,
) -> SelfDevelopmentObjectiveState {
    SelfDevelopmentObjectiveState {
        semantic_progress_rate: semantic_progress_rate(recent_execution_results),
        semantic_no_progress_streak: semantic_no_progress_streak(recent_execution_results),
        consecutive_invalid_plan_batches,
        validation_blocked_by_preconditions: semantic_summary.validation_blocked_by_preconditions,
        compiler_repair_required: semantic_summary.compiler_repair_required,
        misalignment_pressure_score: objective_trend_state.misalignment_pressure_score(),
    }
}

pub fn derive_objective_trend_state(
    planning_attempts: u32,
    invalid_plan_events: u32,
    last_goodness: Option<f32>,
    last_delta_g: Option<f32>,
    recent_execution_results: &[SemanticExecutionResultRecord],
) -> ObjectiveTrendState {
    let mut trend = ObjectiveTrendState {
        planning_attempts,
        invalid_plan_events,
        last_goodness,
        last_delta_g,
        ..ObjectiveTrendState::default()
    };
    trend.record_execution_results(recent_execution_results);
    trend
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticActionIntent {
    BootstrapWorkspace,
    InitCargoProject,
    ValidateCargoCheck,
    CreateEntrypoint(PathBuf),
    CreateModuleFile(PathBuf),
    RestructureModules(PathBuf),
    FixDeadCodeConflict(PathBuf),
    FixUnresolvedImport(PathBuf),
    DefineMissingSymbol(PathBuf),
    ResolveDuplicateDefinition(PathBuf),
    FixTraitBoundFailure(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticExecutionResultRecord {
    pub kind: String,
    pub summary: String,
    pub target_files: Vec<String>,
    pub semantic_progress: bool,
    pub attempted_kind: Option<String>,
}

impl SemanticExecutionResultRecord {
    pub fn new(
        kind: impl Into<String>,
        summary: impl Into<String>,
        target_files: Vec<String>,
        semantic_progress: bool,
    ) -> Self {
        Self {
            kind: kind.into(),
            summary: summary.into(),
            target_files,
            semantic_progress,
            attempted_kind: None,
        }
    }

    pub fn with_attempted_kind(mut self, attempted_kind: impl Into<String>) -> Self {
        self.attempted_kind = Some(attempted_kind.into());
        self
    }

    pub fn render_line(&self) -> String {
        let targets = if self.target_files.is_empty() {
            "none".to_string()
        } else {
            self.target_files.join("|")
        };
        let attempted = self.attempted_kind.as_deref().unwrap_or("none");
        format!(
            "kind={} attempted_kind={} progress={} targets={} summary={}",
            self.kind, attempted, self.semantic_progress, targets, self.summary
        )
    }
}

pub fn classify_planned_action_intents(
    action_kind: &str,
    action_payload: &serde_json::Value,
    target_root: Option<&Path>,
) -> Vec<SemanticActionIntent> {
    let mut out = Vec::new();
    match action_kind {
        "run_command" => {
            let cmd = action_payload.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.contains("cargo new ") {
                out.push(SemanticActionIntent::BootstrapWorkspace);
            }
            if cmd.contains("cargo init") {
                out.push(SemanticActionIntent::InitCargoProject);
            }
            if cmd.contains("cargo check") {
                out.push(SemanticActionIntent::ValidateCargoCheck);
            }
        }
        "edit.rename_symbol" => {
            if let Some(path) = action_payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .map(|path| normalize_path(&path, target_root))
            {
                out.push(SemanticActionIntent::ResolveDuplicateDefinition(path));
            }
        }
        "edit.move_symbol" => {
            if let Some(path) = action_payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .map(|path| normalize_path(&path, target_root))
            {
                out.push(SemanticActionIntent::RestructureModules(path));
            }
        }
        "edit.add_import" => {
            if let Some(path) = action_payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .map(|path| normalize_path(&path, target_root))
            {
                out.push(SemanticActionIntent::FixUnresolvedImport(path));
            }
        }
        "edit.define_symbol_stub" => {
            if let Some(path) = action_payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .map(|path| normalize_path(&path, target_root))
            {
                out.push(SemanticActionIntent::DefineMissingSymbol(path));
            }
        }
        "edit.create_module_file" => {
            if let Some(path) = action_payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .map(|path| normalize_path(&path, target_root))
            {
                out.push(SemanticActionIntent::CreateModuleFile(path));
            }
        }
        "apply_patch" => {
            let patch = action_payload.get("patch").and_then(|v| v.as_str()).unwrap_or("");
            if let Ok(args) = canon_tools_patch::parse_patch(patch) {
                for hunk in args.hunks {
                    match hunk {
                        canon_tools_patch::Hunk::AddFile { path, .. } => {
                            let path = normalize_path(&path, target_root);
                            let text = path.to_string_lossy();
                            if text.ends_with("src/main.rs") || text.ends_with("src/lib.rs") {
                                out.push(SemanticActionIntent::CreateEntrypoint(path));
                            }
                        }
                        canon_tools_patch::Hunk::UpdateFile { path, .. }
                        | canon_tools_patch::Hunk::DeleteFile { path } => {
                            let path = normalize_path(&path, target_root);
                            if patch.contains("allow(dead_code)") {
                                out.push(SemanticActionIntent::FixDeadCodeConflict(path.clone()));
                            }
                            if is_trait_bound_edit(patch) {
                                out.push(SemanticActionIntent::FixTraitBoundFailure(path));
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

pub fn execution_results_for_action(
    intents: &[SemanticActionIntent],
    success: bool,
    stderr: &str,
) -> Vec<SemanticExecutionResultRecord> {
    if !success {
        if intents.is_empty() {
            return vec![SemanticExecutionResultRecord::new(
                "no_semantic_progress",
                format!("action failed: {}", stderr.trim()),
                Vec::new(),
                false,
            )];
        }
        return intents
            .iter()
            .map(|intent| {
                let (kind, targets) = intent_kind_and_targets(intent);
                SemanticExecutionResultRecord::new(
                    "no_semantic_progress",
                    format!("{kind} failed: {}", stderr.trim()),
                    targets,
                    false,
                )
                .with_attempted_kind(kind)
            })
            .collect();
    }
    if intents.is_empty() {
        return vec![SemanticExecutionResultRecord::new(
            "no_semantic_progress",
            "action succeeded without semantic state change classification",
            Vec::new(),
            false,
        )];
    }
    intents
        .iter()
        .map(|intent| match intent {
            SemanticActionIntent::BootstrapWorkspace => SemanticExecutionResultRecord::new(
                "workspace_bootstrapped",
                "workspace bootstrap command succeeded",
                Vec::new(),
                true,
            )
            .with_attempted_kind("bootstrap_workspace"),
            SemanticActionIntent::InitCargoProject => SemanticExecutionResultRecord::new(
                "cargo_project_initialized",
                "cargo project initialization succeeded",
                Vec::new(),
                true,
            )
            .with_attempted_kind("init_cargo_project"),
            SemanticActionIntent::ValidateCargoCheck => SemanticExecutionResultRecord::new(
                "validation_attempted",
                "cargo check executed",
                Vec::new(),
                false,
            )
            .with_attempted_kind("validate_cargo_check"),
            SemanticActionIntent::CreateEntrypoint(path) => SemanticExecutionResultRecord::new(
                "entrypoint_created",
                "entrypoint file created",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("create_entrypoint"),
            SemanticActionIntent::CreateModuleFile(path) => SemanticExecutionResultRecord::new(
                "module_created",
                "module file created",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("create_module_file"),
            SemanticActionIntent::RestructureModules(path) => SemanticExecutionResultRecord::new(
                "module_restructured",
                "module restructure edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("restructure_modules"),
            SemanticActionIntent::FixDeadCodeConflict(path) => SemanticExecutionResultRecord::new(
                "dead_code_conflict_addressed",
                "dead_code conflict edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("fix_dead_code_conflict"),
            SemanticActionIntent::FixUnresolvedImport(path) => SemanticExecutionResultRecord::new(
                "import_resolved",
                "import repair edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("fix_unresolved_import"),
            SemanticActionIntent::DefineMissingSymbol(path) => SemanticExecutionResultRecord::new(
                "symbol_defined",
                "missing symbol definition edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("define_missing_symbol"),
            SemanticActionIntent::ResolveDuplicateDefinition(path) => SemanticExecutionResultRecord::new(
                "duplicate_resolved",
                "duplicate definition repair applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("resolve_duplicate_definition"),
            SemanticActionIntent::FixTraitBoundFailure(path) => SemanticExecutionResultRecord::new(
                "trait_bound_fixed",
                "trait bound repair edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            )
            .with_attempted_kind("fix_trait_bound_failure"),
        })
        .collect()
}

pub fn latest_semantic_progress(results: &[SemanticExecutionResultRecord]) -> bool {
    results
        .iter()
        .rev()
        .next()
        .is_some_and(|result| result.semantic_progress)
}

pub fn latest_graph_proof_verified(results: &[SemanticExecutionResultRecord]) -> bool {
    results.last().is_some_and(|result| result.kind == "graph_proof_verified")
}

pub fn latest_graph_proof_failed(results: &[SemanticExecutionResultRecord]) -> bool {
    results.last().is_some_and(|result| result.kind == "graph_proof_failed")
}

pub fn latest_no_semantic_progress(results: &[SemanticExecutionResultRecord]) -> bool {
    results
        .iter()
        .rev()
        .next()
        .is_some_and(|result| !result.semantic_progress)
}

pub fn semantic_progress_count(results: &[SemanticExecutionResultRecord]) -> usize {
    results.iter().filter(|result| result.semantic_progress).count()
}

pub fn semantic_no_progress_streak(results: &[SemanticExecutionResultRecord]) -> usize {
    results
        .iter()
        .rev()
        .take_while(|result| !result.semantic_progress)
        .count()
}

pub fn semantic_progress_rate(results: &[SemanticExecutionResultRecord]) -> f32 {
    if results.is_empty() {
        return 0.0;
    }
    semantic_progress_count(results) as f32 / results.len() as f32
}

fn render_bullets(lines: &[String]) -> String {
    if lines.is_empty() {
        "- none".to_string()
    } else {
        lines
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn intent_kind_and_targets(intent: &SemanticActionIntent) -> (&'static str, Vec<String>) {
    match intent {
        SemanticActionIntent::BootstrapWorkspace => ("bootstrap_workspace", Vec::new()),
        SemanticActionIntent::InitCargoProject => ("init_cargo_project", Vec::new()),
        SemanticActionIntent::ValidateCargoCheck => ("validate_cargo_check", Vec::new()),
        SemanticActionIntent::CreateEntrypoint(path) => {
            ("create_entrypoint", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::CreateModuleFile(path) => {
            ("create_module_file", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::RestructureModules(path) => {
            ("restructure_modules", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::FixDeadCodeConflict(path) => {
            ("fix_dead_code_conflict", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::FixUnresolvedImport(path) => {
            ("fix_unresolved_import", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::DefineMissingSymbol(path) => {
            ("define_missing_symbol", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::ResolveDuplicateDefinition(path) => {
            ("resolve_duplicate_definition", vec![path.to_string_lossy().to_string()])
        }
        SemanticActionIntent::FixTraitBoundFailure(path) => {
            ("fix_trait_bound_failure", vec![path.to_string_lossy().to_string()])
        }
    }
}

fn normalize_path(path: &Path, target_root: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        target_root.map(|root| root.join(path)).unwrap_or_else(|| path.to_path_buf())
    }
}

fn is_trait_bound_edit(patch: &str) -> bool {
    patch.lines().any(|line| {
        if !(line.starts_with('+') || line.starts_with('-')) {
            return false;
        }
        let content = line[1..].trim_start();
        content.contains("impl ")
            || content.contains("where ")
            || content.contains("derive(")
            || content.contains(": ")
    })
}

fn parse_compiler_hint_record(line: &str) -> Option<CompilerHintRecord> {
    let kind = parse_field(line, "kind=")?;
    let targets = parse_field(line, "targets=").unwrap_or_else(|| "none".to_string());
    let summary = parse_field(line, "summary=").unwrap_or_default();
    let repair = parse_field(line, "repair=").unwrap_or_default();
    let target_files = if targets == "none" || targets.is_empty() {
        Vec::new()
    } else {
        targets.split('|').map(|s| s.trim().to_string()).collect()
    };
    Some(CompilerHintRecord {
        kind,
        summary,
        suggested_repair: repair,
        target_files,
    })
}

fn parse_field(line: &str, marker: &str) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let delimiter = next_field_delimiter(marker);
    let end = if delimiter.is_empty() {
        tail.len()
    } else {
        tail.find(delimiter).unwrap_or(tail.len())
    };
    Some(tail[..end].trim().to_string())
}

fn next_field_delimiter(marker: &str) -> &'static str {
    match marker {
        "kind=" => " targets=",
        "targets=" => " summary=",
        "summary=" => " repair=",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        derive_development_objectives, derive_self_development_objective_state,
        primary_development_strategy_kind, CompilerHintKind, CompilerHintRecord,
        DevelopmentObjectiveKind, DevelopmentStrategyKind, ObjectiveTrendState,
        SemanticStateSummary,
    };

    #[test]
    fn round_trip_workspace_facts() {
        let summary = SemanticStateSummary {
            version: SemanticStateSummary::VERSION,
            complete: true,
            target_root: Some("/tmp/example".into()),
            path_exists: true,
            repo_initialized: false,
            cargo_project: true,
            crate_name: Some("example".into()),
            entrypoint_kind: Some("lib".into()),
            rust_file_count: Some(2),
            source_files: vec!["src/lib.rs".into()],
            module_gaps: vec!["index -> src/index.rs".into()],
            planning_preconditions: vec!["must_create_missing_modules=true".into()],
            repair_intents: vec!["repair_intent=create_missing_modules".into()],
            compiler_hints: vec![CompilerHintRecord::new(
                CompilerHintKind::MissingModule,
                "compiler reports missing module `index`",
                "create the missing module file",
                vec!["src/lib.rs".into()],
            )],
            validation_blocked_by_preconditions: true,
            compiler_repair_required: true,
            ..SemanticStateSummary::default()
        };
        let restored = SemanticStateSummary::from_workspace_facts(&summary.to_workspace_facts());
        assert_eq!(restored, summary);
    }

    #[test]
    fn render_blocks_include_key_sections() {
        let summary = SemanticStateSummary {
            version: SemanticStateSummary::VERSION,
            complete: true,
            path_exists: true,
            cargo_project: true,
            planning_preconditions: vec!["must_create_entrypoint=true".into()],
            repair_intents: vec!["repair_intent=create_entrypoint priority=3".into()],
            compiler_hints: vec![CompilerHintRecord::new(
                CompilerHintKind::MissingSymbol,
                "compiler cannot find `run` in scope",
                "define the missing symbol or import it before cargo check",
                vec!["src/main.rs".into()],
            )],
            ..SemanticStateSummary::default()
        };
        assert!(summary.render_planner_block().contains("Planning preconditions:"));
        assert!(summary.render_planner_block().contains("Compiler hints:"));
        assert!(summary.render_route_block().contains("Semantic summary:"));
        assert!(summary.compact_block().contains("compiler_hint_kinds=missing_symbol"));
    }

    #[test]
    fn actionable_compiler_hints_are_detected() {
        let summary = SemanticStateSummary {
            compiler_hints: vec![CompilerHintRecord::new(
                CompilerHintKind::DuplicateDefinition,
                "compiler reports duplicate definition",
                "remove duplicate",
                vec!["src/lib.rs".into()],
            )],
            ..SemanticStateSummary::default()
        };
        assert!(summary.has_actionable_compiler_hints());
    }

    #[test]
    fn development_objectives_prioritize_contradictions_when_present() {
        let summary = SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState {
            route_objective_contradiction_events: 3,
            goal_objective_drift_events: 2,
            ..ObjectiveTrendState::default()
        };
        let objective_state = derive_self_development_objective_state(&summary, 0, &[], &trend);
        let objectives = derive_development_objectives(&summary, &objective_state, &trend);
        assert_eq!(objectives[0].kind, DevelopmentObjectiveKind::ReduceContradictionRate);
        assert!(objectives[0].priority_score > 0);
    }

    #[test]
    fn development_strategy_prefers_config_fix_for_dead_code_conflict() {
        let summary = SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            compiler_hints: vec![CompilerHintRecord::new(
                CompilerHintKind::DeadCodeForbidConflict,
                "compiler forbids dead_code while source adds allow(dead_code)",
                "edit config or lint policy before more source edits",
                vec![".cargo/config.toml".into()],
            )],
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState::default();
        let objective_state = derive_self_development_objective_state(&summary, 0, &[], &trend);
        assert_eq!(
            primary_development_strategy_kind(&objective_state, &trend, &summary),
            DevelopmentStrategyKind::FixConfigLintPolicy
        );
    }

    #[test]
    fn development_strategy_prefers_graph_backed_rename_for_duplicate_definition() {
        let summary = SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            graph_artifact_id: Some("artifact".into()),
            compiler_hints: vec![CompilerHintRecord::new(
                CompilerHintKind::DuplicateDefinition,
                "compiler reports duplicate definition",
                "rename the duplicate definition",
                vec!["src/lib.rs".into()],
            )],
            ..SemanticStateSummary::default()
        };
        let trend = ObjectiveTrendState::default();
        let objective_state = derive_self_development_objective_state(&summary, 0, &[], &trend);
        assert_eq!(
            primary_development_strategy_kind(&objective_state, &trend, &summary),
            DevelopmentStrategyKind::PlanSymbolAwareRename
        );
    }
}
