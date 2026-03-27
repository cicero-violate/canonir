use serde::{Deserialize, Serialize};

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
    let end = tail.find(next_field_delimiter(marker)).unwrap_or(tail.len());
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
