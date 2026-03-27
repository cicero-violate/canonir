use serde::{Deserialize, Serialize};

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
    pub compiler_hints: Vec<String>,
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
            facts.push(format!("semantic.compiler_hint={hint}"));
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
                summary.compiler_hints.push(value.to_string());
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
            format!(
                "crate_name={}",
                self.crate_name.as_deref().unwrap_or("NA")
            ),
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
        parts.join("\n")
    }

    pub fn render_planner_block(&self) -> String {
        format!(
            "Environment model:\n{}\n\nPlanning preconditions:\n{}\n\nRepair intents:\n{}\n\nSemantic summary:\n{}",
            render_bullets(&self.planner_lines()),
            render_bullets(&self.planning_preconditions),
            render_bullets(&self.repair_intents),
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

#[cfg(test)]
mod tests {
    use super::SemanticStateSummary;

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
            compiler_hints: vec!["compiler reports missing module `index`".into()],
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
            ..SemanticStateSummary::default()
        };
        assert!(summary.render_planner_block().contains("Planning preconditions:"));
        assert!(summary.render_route_block().contains("Semantic summary:"));
    }
}
