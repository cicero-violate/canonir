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
        facts
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

impl LlmSemanticContext {
    pub fn render_goal_gen_block(&self) -> String {
        let mut lines = Vec::new();
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
        format!("LLM semantic context:
{}", render_bullets(&lines))
    }

    pub fn render_router_block(&self) -> String {
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
        format!("LLM semantic context:
{}", render_bullets(&lines))
    }

    pub fn render_planner_base_block(&self) -> String {
        let mut sections = vec![self.semantic_summary.render_planner_block()];
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
        }
        sections.join("

")
    }

    pub fn render_planner_delta_block(&self) -> String {
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
        sections.join("

")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticActionIntent {
    BootstrapWorkspace,
    InitCargoProject,
    ValidateCargoCheck,
    CreateEntrypoint(PathBuf),
    CreateModuleFile(PathBuf),
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
        }
    }

    pub fn render_line(&self) -> String {
        let targets = if self.target_files.is_empty() {
            "none".to_string()
        } else {
            self.target_files.join("|")
        };
        format!(
            "kind={} progress={} targets={} summary={}",
            self.kind, self.semantic_progress, targets, self.summary
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
                            } else if text.ends_with(".rs") {
                                out.push(SemanticActionIntent::CreateModuleFile(path));
                            }
                        }
                        canon_tools_patch::Hunk::UpdateFile { path, .. }
                        | canon_tools_patch::Hunk::DeleteFile { path } => {
                            let path = normalize_path(&path, target_root);
                            if patch.contains("allow(dead_code)") {
                                out.push(SemanticActionIntent::FixDeadCodeConflict(path.clone()));
                            }
                            if is_import_edit(patch) {
                                out.push(SemanticActionIntent::FixUnresolvedImport(path.clone()));
                            }
                            if is_missing_symbol_edit(patch) {
                                out.push(SemanticActionIntent::DefineMissingSymbol(path.clone()));
                            }
                            if is_duplicate_definition_edit(patch) {
                                out.push(SemanticActionIntent::ResolveDuplicateDefinition(path.clone()));
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
            ),
            SemanticActionIntent::InitCargoProject => SemanticExecutionResultRecord::new(
                "cargo_project_initialized",
                "cargo project initialization succeeded",
                Vec::new(),
                true,
            ),
            SemanticActionIntent::ValidateCargoCheck => SemanticExecutionResultRecord::new(
                "validation_attempted",
                "cargo check executed",
                Vec::new(),
                false,
            ),
            SemanticActionIntent::CreateEntrypoint(path) => SemanticExecutionResultRecord::new(
                "entrypoint_created",
                "entrypoint file created",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::CreateModuleFile(path) => SemanticExecutionResultRecord::new(
                "module_created",
                "module file created",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::FixDeadCodeConflict(path) => SemanticExecutionResultRecord::new(
                "dead_code_conflict_addressed",
                "dead_code conflict edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::FixUnresolvedImport(path) => SemanticExecutionResultRecord::new(
                "import_resolved",
                "import repair edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::DefineMissingSymbol(path) => SemanticExecutionResultRecord::new(
                "symbol_defined",
                "missing symbol definition edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::ResolveDuplicateDefinition(path) => SemanticExecutionResultRecord::new(
                "duplicate_resolved",
                "duplicate definition repair applied",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
            SemanticActionIntent::FixTraitBoundFailure(path) => SemanticExecutionResultRecord::new(
                "trait_bound_fixed",
                "trait bound repair edit applied",
                vec![path.to_string_lossy().to_string()],
                true,
            ),
        })
        .collect()
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

fn is_import_edit(patch: &str) -> bool {
    patch.contains("use ")
        || patch.contains("mod ")
        || patch.contains("pub use ")
        || patch.contains("extern crate ")
}

fn is_missing_symbol_edit(patch: &str) -> bool {
    (patch.contains("fn ") && !patch.contains("fn main"))
        || patch.contains("struct ")
        || patch.contains("enum ")
        || patch.contains("type ")
        || patch.contains("const ")
        || patch.contains("let ")
        || patch.contains("impl ")
        || patch.contains("use ")
}

fn is_duplicate_definition_edit(patch: &str) -> bool {
    patch.contains("rename") || has_definition_edit(patch)
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

fn has_definition_edit(patch: &str) -> bool {
    patch.lines().any(|line| {
        if !(line.starts_with('+') || line.starts_with('-')) {
            return false;
        }
        let content = line[1..].trim_start();
        let content = content
            .strip_prefix("pub(crate) ")
            .or_else(|| content.strip_prefix("pub "))
            .unwrap_or(content);
        content.starts_with("fn ")
            || content.starts_with("struct ")
            || content.starts_with("enum ")
            || content.starts_with("type ")
            || content.starts_with("const ")
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
    use super::{CompilerHintKind, CompilerHintRecord, SemanticStateSummary};

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
}
