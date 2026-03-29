use canon_invariant::{
    evaluate_constraint_context, meta_invariant_classify_bootstrap_tool, ConstraintAction,
    ConstraintContext, ConstraintDecision, ConstraintState,
    MetaInvariantBootstrapToolChoice,
};
use canon_semantic_state::SemanticStateSummary;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecState {
    pub target_root: PathBuf,
    pub semantic_path_exists: bool,
    pub semantic_cargo_project: bool,
    pub real_path_exists: bool,
    pub real_cargo_project: bool,
    pub actionable_failure: bool,
    pub validation_blocked: bool,
    pub entrypoint_missing: bool,
    pub module_gaps_present: bool,
    pub failure_class_no_actionable: bool,
    pub failure_scope_localized: bool,
    pub failure_scope_workspace: bool,
    pub failure_scope_tooling: bool,
}

impl ExecState {
    pub fn from_semantic_summary(target_root: &Path, summary: &SemanticStateSummary) -> Self {
        let real_cargo_project = target_root.join("Cargo.toml").exists();
        let real_entrypoint_missing =
            !target_root.join("src/main.rs").exists() && !target_root.join("src/lib.rs").exists();
        let failure_scope_localized = summary.failure_scope.as_deref() == Some("localized")
            || !summary.module_gaps.is_empty()
            || summary.compiler_hints.iter().any(|hint| {
                hint.kind_enum().is_some_and(|kind| {
                    matches!(
                        kind,
                        canon_semantic_state::CompilerHintKind::MissingModule
                            | canon_semantic_state::CompilerHintKind::DeadCodeForbidConflict
                            | canon_semantic_state::CompilerHintKind::MissingEntrypoint
                            | canon_semantic_state::CompilerHintKind::UnresolvedImport
                            | canon_semantic_state::CompilerHintKind::MissingSymbol
                            | canon_semantic_state::CompilerHintKind::DuplicateDefinition
                            | canon_semantic_state::CompilerHintKind::TraitBoundFailure
                    )
                }) && hint
                    .target_files
                    .iter()
                    .any(|path| !path.trim().is_empty() && path != "none")
            });
        Self {
            target_root: target_root.to_path_buf(),
            semantic_path_exists: summary.path_exists,
            semantic_cargo_project: summary.cargo_project,
            real_path_exists: target_root.exists(),
            real_cargo_project,
            actionable_failure: summary.validation_blocked_by_preconditions
                || summary.compiler_repair_required
                || !summary.planning_preconditions.is_empty()
                || !summary.module_gaps.is_empty()
                || summary.has_actionable_compiler_hints()
                || summary
                    .primary_failure_class()
                    .as_deref()
                    .is_some_and(|class| class != "no_actionable_failure"),
            validation_blocked: summary.validation_blocked_by_preconditions,
            entrypoint_missing: matches!(summary.entrypoint_kind.as_deref(), Some("none") | None)
                && summary.cargo_project
                && real_cargo_project
                && real_entrypoint_missing,
            module_gaps_present: !summary.module_gaps.is_empty(),
            failure_class_no_actionable: summary.primary_failure_class().as_deref() == Some("no_actionable_failure"),
            failure_scope_localized,
            failure_scope_workspace: summary.failure_scope.as_deref() == Some("workspace"),
            failure_scope_tooling: summary.failure_scope.as_deref() == Some("tooling"),
        }
    }

    pub fn constraint_state(&self) -> ConstraintState {
        ConstraintState {
            semantic_path_exists: self.semantic_path_exists,
            semantic_cargo_project: self.semantic_cargo_project,
            real_path_exists: self.real_path_exists,
            real_cargo_project: self.real_cargo_project,
            actionable_failure: self.actionable_failure,
            validation_blocked: self.validation_blocked,
            entrypoint_missing: self.entrypoint_missing,
            module_gaps_present: self.module_gaps_present,
            recent_no_semantic_progress: false,
            failure_class_no_actionable: self.failure_class_no_actionable,
            failure_scope_localized: self.failure_scope_localized,
            failure_scope_workspace: self.failure_scope_workspace,
            failure_scope_tooling: self.failure_scope_tooling,
            route_objective_contradiction: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecAction {
    RunCommand { cmd: String, cwd: PathBuf },
    Other { action_kind: String },
}

impl ExecAction {
    pub fn from_planned(action_kind: &str, action_payload: &serde_json::Value) -> Self {
        if action_kind == "run_command" {
            let cmd = action_payload
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let cwd = action_payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            Self::RunCommand { cmd, cwd }
        } else {
            Self::Other {
                action_kind: action_kind.to_string(),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecDecision {
    Allow,
    Forbid(&'static str),
    Rewrite(ExecAction, &'static str),
}

pub fn validate_exec_action(state: &ExecState, action: &ExecAction) -> ExecDecision {
    let constraint_action = match action {
        ExecAction::RunCommand { cmd, .. } => match meta_invariant_classify_bootstrap_tool(cmd) {
            Some(MetaInvariantBootstrapToolChoice::CargoInit) => Some(ConstraintAction::CargoInit),
            Some(MetaInvariantBootstrapToolChoice::CargoNew) => Some(ConstraintAction::CargoNew),
            None if cmd.contains("cargo check") || cmd.contains("cargo build") || cmd.contains("cargo test") => {
                Some(ConstraintAction::Validation)
            }
            None => Some(ConstraintAction::RepairWorkspace),
        },
        ExecAction::Other { .. } => Some(ConstraintAction::Other),
    };
    match evaluate_constraint_context(&ConstraintContext {
        state: state.constraint_state(),
        route: None,
        action: constraint_action,
        deterministic_route: None,
    }) {
        ConstraintDecision::Allow => ExecDecision::Allow,
        ConstraintDecision::Forbid(reason) => ExecDecision::Forbid(reason),
        ConstraintDecision::RewriteAction(ConstraintAction::CargoNew, reason) => {
            let ExecAction::RunCommand { cmd, .. } = action else {
                return ExecDecision::Forbid(reason);
            };
            ExecDecision::Rewrite(
                ExecAction::RunCommand {
                    cmd: rewrite_cargo_init_to_new(cmd, &state.target_root),
                    cwd: state
                        .target_root
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| state.target_root.clone()),
                },
                reason,
            )
        }
        ConstraintDecision::RewriteAction(ConstraintAction::CargoInit, reason) => {
            let ExecAction::RunCommand { cmd, .. } = action else {
                return ExecDecision::Forbid(reason);
            };
            ExecDecision::Rewrite(
                ExecAction::RunCommand {
                    cmd: rewrite_cargo_new_to_init(cmd),
                    cwd: state.target_root.clone(),
                },
                reason,
            )
        }
        ConstraintDecision::RewriteAction(_, reason) => ExecDecision::Forbid(reason),
        ConstraintDecision::RewriteRoute(_, reason) => ExecDecision::Forbid(reason),
    }
}

fn rewrite_cargo_init_to_new(cmd: &str, target_root: &Path) -> String {
    let target_name = target_root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("workspace");
    let mut parts = vec!["cargo".to_string(), "new".to_string()];
    if let Some(name) = extract_flag_value(cmd, "--name") {
        parts.push("--name".to_string());
        parts.push(name);
    }
    if cmd.contains("--lib") {
        parts.push("--lib".to_string());
    } else if cmd.contains("--bin") {
        parts.push("--bin".to_string());
    }
    parts.push(target_name.to_string());
    parts.join(" ")
}

fn rewrite_cargo_new_to_init(cmd: &str) -> String {
    let mut parts = vec!["cargo".to_string(), "init".to_string()];
    if let Some(name) = extract_flag_value(cmd, "--name").or_else(|| extract_cargo_new_target(cmd)) {
        parts.push("--name".to_string());
        parts.push(name);
    }
    if cmd.contains("--lib") {
        parts.push("--lib".to_string());
    } else if cmd.contains("--bin") {
        parts.push("--bin".to_string());
    }
    parts.push(".".to_string());
    parts.join(" ")
}

fn extract_flag_value(cmd: &str, flag: &str) -> Option<String> {
    let mut tokens = cmd.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == flag {
            return tokens.next().map(|value| value.to_string());
        }
    }
    None
}

fn extract_cargo_new_target(cmd: &str) -> Option<String> {
    let tokens = cmd.split_whitespace().collect::<Vec<_>>();
    for token in tokens.into_iter().rev() {
        if !token.starts_with('-') && token != "new" && token != "cargo" {
            return Some(token.trim_end_matches('/').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RealWorkspaceState {
        Missing,
        ExistingNonCargo,
        ExistingCargo,
    }

    impl RealWorkspaceState {
        const ALL: [Self; 3] = [Self::Missing, Self::ExistingNonCargo, Self::ExistingCargo];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SemanticWorkspaceState {
        Missing,
        ExistingNonCargo,
        ExistingCargo,
    }

    impl SemanticWorkspaceState {
        const ALL: [Self; 3] = [Self::Missing, Self::ExistingNonCargo, Self::ExistingCargo];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ActionCase {
        CargoInit,
        CargoNew,
        CargoCheck,
    }

    impl ActionCase {
        const ALL: [Self; 3] = [Self::CargoInit, Self::CargoNew, Self::CargoCheck];
    }

    fn setup_root(real: RealWorkspaceState) -> PathBuf {
        let root = std::env::temp_dir().join(format!("canon_exec_constraints_{}", uuid::Uuid::new_v4()));
        match real {
            RealWorkspaceState::Missing => {}
            RealWorkspaceState::ExistingNonCargo => {
                std::fs::create_dir_all(&root).unwrap();
            }
            RealWorkspaceState::ExistingCargo => {
                std::fs::create_dir_all(root.join("src")).unwrap();
                std::fs::write(
                    root.join("Cargo.toml"),
                    "[package]\nname = \"exec_constraints\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
                )
                .unwrap();
                std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
            }
        }
        root
    }

    fn semantic_summary(semantic: SemanticWorkspaceState) -> SemanticStateSummary {
        let (path_exists, cargo_project, entrypoint_kind) = match semantic {
            SemanticWorkspaceState::Missing => (false, false, None),
            SemanticWorkspaceState::ExistingNonCargo => (true, false, None),
            SemanticWorkspaceState::ExistingCargo => (true, true, Some("bin".to_string())),
        };
        SemanticStateSummary {
            complete: true,
            path_exists,
            cargo_project,
            entrypoint_kind,
            ..SemanticStateSummary::default()
        }
    }

    fn action(case: ActionCase, root: &Path) -> ExecAction {
        match case {
            ActionCase::CargoInit => ExecAction::RunCommand {
                cmd: "cargo init --name event_sim_coverage .".to_string(),
                cwd: root.to_path_buf(),
            },
            ActionCase::CargoNew => ExecAction::RunCommand {
                cmd: "cargo new event_sim_coverage".to_string(),
                cwd: root.to_path_buf(),
            },
            ActionCase::CargoCheck => ExecAction::RunCommand {
                cmd: "cargo check".to_string(),
                cwd: root.to_path_buf(),
            },
        }
    }

    fn expected_decision(real: RealWorkspaceState, action: ActionCase) -> &'static str {
        match (real, action) {
            (RealWorkspaceState::Missing, ActionCase::CargoInit) => "rewrite_new",
            (RealWorkspaceState::Missing, ActionCase::CargoCheck) => "forbid_validate_missing",
            (RealWorkspaceState::Missing, _) => "allow",
            (RealWorkspaceState::ExistingNonCargo, ActionCase::CargoNew) => "rewrite_init",
            (RealWorkspaceState::ExistingNonCargo, ActionCase::CargoCheck) => "allow",
            (RealWorkspaceState::ExistingNonCargo, _) => "allow",
            (RealWorkspaceState::ExistingCargo, ActionCase::CargoInit | ActionCase::CargoNew) => "forbid",
            (RealWorkspaceState::ExistingCargo, ActionCase::CargoCheck) => "allow",
        }
    }

    #[test]
    fn bootstrap_constraint_state_map_is_exhaustive() {
        for real in RealWorkspaceState::ALL {
            for semantic in SemanticWorkspaceState::ALL {
                for action_case in ActionCase::ALL {
                    let root = setup_root(real);
                    let state = ExecState::from_semantic_summary(&root, &semantic_summary(semantic));
                    let decision = validate_exec_action(&state, &action(action_case, &root));
                    match expected_decision(real, action_case) {
                        "allow" => assert_eq!(decision, ExecDecision::Allow, "real={real:?} semantic={semantic:?} action={action_case:?}"),
                        "rewrite_new" => match decision {
                            ExecDecision::Rewrite(ExecAction::RunCommand { cmd, cwd }, _) => {
                                assert!(cmd.contains("cargo new"), "real={real:?} semantic={semantic:?} action={action_case:?}");
                                assert_eq!(cwd, root.parent().unwrap_or(root.as_path()));
                            }
                            other => panic!("expected rewrite to cargo new, got {other:?} for real={real:?} semantic={semantic:?} action={action_case:?}"),
                        },
                        "rewrite_init" => match decision {
                            ExecDecision::Rewrite(ExecAction::RunCommand { cmd, cwd }, _) => {
                                assert!(cmd.contains("cargo init"), "real={real:?} semantic={semantic:?} action={action_case:?}");
                                assert_eq!(cwd, root);
                            }
                            other => panic!("expected rewrite to cargo init, got {other:?} for real={real:?} semantic={semantic:?} action={action_case:?}"),
                        },
                        "forbid" => match decision {
                            ExecDecision::Forbid(reason) => {
                                assert!(reason.contains("bootstrap"), "real={real:?} semantic={semantic:?} action={action_case:?}");
                            }
                            other => panic!("expected forbid, got {other:?} for real={real:?} semantic={semantic:?} action={action_case:?}"),
                        },
                        "forbid_validate_missing" => match decision {
                            ExecDecision::Forbid(reason) => {
                                assert!(reason.contains("validation actions are forbidden"), "real={real:?} semantic={semantic:?} action={action_case:?}");
                            }
                            other => panic!("expected validation forbid, got {other:?} for real={real:?} semantic={semantic:?} action={action_case:?}"),
                        },
                        _ => unreachable!(),
                    }
                }
            }
        }
    }

    #[test]
    fn validation_is_forbidden_when_semantic_state_requires_entrypoint_or_modules() {
        let root = setup_root(RealWorkspaceState::ExistingCargo);
        let summary = SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            entrypoint_kind: Some("none".into()),
            module_gaps: vec!["index -> src/index.rs".into()],
            validation_blocked_by_preconditions: true,
            ..SemanticStateSummary::default()
        };
        let state = ExecState::from_semantic_summary(&root, &summary);
        let decision = validate_exec_action(&state, &action(ActionCase::CargoCheck, &root));
        assert!(matches!(decision, ExecDecision::Forbid(reason) if reason.contains("validation actions are forbidden")));
    }
}
