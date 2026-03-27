use crate::compiler_hints::extract_compiler_hints;
use crate::env_model::{EntrypointKind, WorkspaceModel};
use canon_semantic_state::{CompilerHintKind, SemanticStateSummary};
use canon_tools_patch::parse_patch;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanningPrecondition {
    MustBootstrapWorkspace,
    MustInitCargoProject,
    MustCreateEntrypoint,
    MustCreateMissingModules,
    MustFixDeadCodeForbidConflict,
    MustFixUnresolvedImport,
    MustDefineMissingSymbol,
    MustResolveDuplicateDefinition,
    MustFixTraitBoundFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepairIntent {
    BootstrapWorkspace,
    InitCargoProject,
    CreateEntrypoint,
    CreateMissingModules,
    FixDeadCodeForbidConflict,
    FixUnresolvedImport,
    DefineMissingSymbol,
    ResolveDuplicateDefinition,
    FixTraitBoundFailure,
}

pub fn derive_preconditions(
    workspace_model: Option<&WorkspaceModel>,
    compiler_errors: &[serde_json::Value],
) -> Vec<PlanningPrecondition> {
    let mut out = Vec::new();
    if let Some(model) = workspace_model {
        if !model.path_exists {
            out.push(PlanningPrecondition::MustBootstrapWorkspace);
            return out;
        }
        if !model.cargo_toml_exists {
            out.push(PlanningPrecondition::MustInitCargoProject);
        }
        if model.cargo_toml_exists && model.entrypoint_kind == EntrypointKind::None {
            out.push(PlanningPrecondition::MustCreateEntrypoint);
        }
        if !model.module_gaps.is_empty() {
            out.push(PlanningPrecondition::MustCreateMissingModules);
        }
    }

    for hint in extract_compiler_hints(compiler_errors) {
        let Some(kind) = hint.kind_enum() else {
            continue;
        };
        if kind == CompilerHintKind::DeadCodeForbidConflict
            && !out.contains(&PlanningPrecondition::MustFixDeadCodeForbidConflict)
        {
            out.push(PlanningPrecondition::MustFixDeadCodeForbidConflict);
        } else if kind == CompilerHintKind::UnresolvedImport
            && !out.contains(&PlanningPrecondition::MustFixUnresolvedImport)
        {
            out.push(PlanningPrecondition::MustFixUnresolvedImport);
        } else if kind == CompilerHintKind::MissingSymbol
            && !out.contains(&PlanningPrecondition::MustDefineMissingSymbol)
        {
            out.push(PlanningPrecondition::MustDefineMissingSymbol);
        } else if kind == CompilerHintKind::DuplicateDefinition
            && !out.contains(&PlanningPrecondition::MustResolveDuplicateDefinition)
        {
            out.push(PlanningPrecondition::MustResolveDuplicateDefinition);
        } else if kind == CompilerHintKind::TraitBoundFailure
            && !out.contains(&PlanningPrecondition::MustFixTraitBoundFailure)
        {
            out.push(PlanningPrecondition::MustFixTraitBoundFailure);
        }
    }
    out
}

pub fn planner_lines(preconditions: &[PlanningPrecondition]) -> Vec<String> {
    preconditions
        .iter()
        .map(|precondition| match precondition {
            PlanningPrecondition::MustBootstrapWorkspace => {
                "must_bootstrap_workspace=true repair=cargo_init_or_create_workspace".to_string()
            }
            PlanningPrecondition::MustInitCargoProject => {
                "must_init_cargo_project=true repair=prefer_cargo_init".to_string()
            }
            PlanningPrecondition::MustCreateEntrypoint => {
                "must_create_entrypoint=true repair=create_src_main_or_lib_before_cargo_check".to_string()
            }
            PlanningPrecondition::MustCreateMissingModules => {
                "must_create_missing_modules=true repair=create_declared_module_files_before_cargo_check".to_string()
            }
            PlanningPrecondition::MustFixDeadCodeForbidConflict => {
                "must_fix_dead_code_forbid_conflict=true repair=remove_allow_dead_code_or_make_code_used".to_string()
            }
            PlanningPrecondition::MustFixUnresolvedImport => {
                "must_fix_unresolved_import=true repair=edit_import_or_define_missing_import_target".to_string()
            }
            PlanningPrecondition::MustDefineMissingSymbol => {
                "must_define_missing_symbol=true repair=define_or_import_missing_symbol".to_string()
            }
            PlanningPrecondition::MustResolveDuplicateDefinition => {
                "must_resolve_duplicate_definition=true repair=remove_or_rename_duplicate_definition".to_string()
            }
            PlanningPrecondition::MustFixTraitBoundFailure => {
                "must_fix_trait_bound_failure=true repair=edit_local_type_impl_or_callsite_for_trait_bound".to_string()
            }
        })
        .collect()
}

pub fn derive_repair_intents(preconditions: &[PlanningPrecondition]) -> Vec<RepairIntent> {
    let mut intents = Vec::new();
    for precondition in preconditions {
        let intent = match precondition {
            PlanningPrecondition::MustBootstrapWorkspace => RepairIntent::BootstrapWorkspace,
            PlanningPrecondition::MustInitCargoProject => RepairIntent::InitCargoProject,
            PlanningPrecondition::MustCreateEntrypoint => RepairIntent::CreateEntrypoint,
            PlanningPrecondition::MustCreateMissingModules => RepairIntent::CreateMissingModules,
            PlanningPrecondition::MustFixDeadCodeForbidConflict => RepairIntent::FixDeadCodeForbidConflict,
            PlanningPrecondition::MustFixUnresolvedImport => RepairIntent::FixUnresolvedImport,
            PlanningPrecondition::MustDefineMissingSymbol => RepairIntent::DefineMissingSymbol,
            PlanningPrecondition::MustResolveDuplicateDefinition => RepairIntent::ResolveDuplicateDefinition,
            PlanningPrecondition::MustFixTraitBoundFailure => RepairIntent::FixTraitBoundFailure,
        };
        if !intents.contains(&intent) {
            intents.push(intent);
        }
    }
    intents
}

pub fn derive_preconditions_from_lines(lines: &[String]) -> Vec<PlanningPrecondition> {
    let mut out = Vec::new();
    for line in lines {
        let precondition = if line.contains("must_bootstrap_workspace=true") {
            Some(PlanningPrecondition::MustBootstrapWorkspace)
        } else if line.contains("must_init_cargo_project=true") {
            Some(PlanningPrecondition::MustInitCargoProject)
        } else if line.contains("must_create_entrypoint=true") {
            Some(PlanningPrecondition::MustCreateEntrypoint)
        } else if line.contains("must_create_missing_modules=true") {
            Some(PlanningPrecondition::MustCreateMissingModules)
        } else if line.contains("must_fix_dead_code_forbid_conflict=true") {
            Some(PlanningPrecondition::MustFixDeadCodeForbidConflict)
        } else if line.contains("must_fix_unresolved_import=true") {
            Some(PlanningPrecondition::MustFixUnresolvedImport)
        } else if line.contains("must_define_missing_symbol=true") {
            Some(PlanningPrecondition::MustDefineMissingSymbol)
        } else if line.contains("must_resolve_duplicate_definition=true") {
            Some(PlanningPrecondition::MustResolveDuplicateDefinition)
        } else if line.contains("must_fix_trait_bound_failure=true") {
            Some(PlanningPrecondition::MustFixTraitBoundFailure)
        } else {
            None
        };
        if let Some(precondition) = precondition {
            if !out.contains(&precondition) {
                out.push(precondition);
            }
        }
    }
    out
}

pub fn repair_intent_lines(intents: &[RepairIntent]) -> Vec<String> {
    intents
        .iter()
        .map(|intent| match intent {
            RepairIntent::BootstrapWorkspace => {
                "repair_intent=bootstrap_workspace priority=1 first_batch=create_or_init_workspace".to_string()
            }
            RepairIntent::InitCargoProject => {
                "repair_intent=init_cargo_project priority=2 first_batch=run_cargo_init".to_string()
            }
            RepairIntent::CreateEntrypoint => {
                "repair_intent=create_entrypoint priority=3 first_batch=create_src_main_or_lib".to_string()
            }
            RepairIntent::CreateMissingModules => {
                "repair_intent=create_missing_modules priority=4 first_batch=create_declared_module_files".to_string()
            }
            RepairIntent::FixDeadCodeForbidConflict => {
                "repair_intent=fix_dead_code_forbid_conflict priority=5 first_batch=edit_conflicting_allow_dead_code".to_string()
            }
            RepairIntent::FixUnresolvedImport => {
                "repair_intent=fix_unresolved_import priority=6 first_batch=edit_import_or_define_target".to_string()
            }
            RepairIntent::DefineMissingSymbol => {
                "repair_intent=define_missing_symbol priority=7 first_batch=define_or_import_symbol".to_string()
            }
            RepairIntent::ResolveDuplicateDefinition => {
                "repair_intent=resolve_duplicate_definition priority=8 first_batch=remove_or_rename_duplicate".to_string()
            }
            RepairIntent::FixTraitBoundFailure => {
                "repair_intent=fix_trait_bound_failure priority=9 first_batch=edit_type_impl_or_callsite".to_string()
            }
        })
        .collect()
}

pub fn validate_preconditions(
    actions: &[canon_event::LoopPlanned],
    target_root: &Path,
    preconditions: &[PlanningPrecondition],
    semantic_summary: &SemanticStateSummary,
) -> Result<(), String> {
    let intents = derive_repair_intents(preconditions);
    if preconditions.contains(&PlanningPrecondition::MustBootstrapWorkspace) && !contains_bootstrap_action(actions) {
        return Err("target workspace is missing; first plan must create/init the workspace".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustInitCargoProject) && !contains_cargo_init(actions) {
        return Err("target directory exists but is not a Cargo project; first plan must initialize Cargo".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustCreateEntrypoint)
        && contains_cargo_check(actions)
        && !contains_entrypoint_creation(actions, target_root)
    {
        return Err("cargo check planned before creating src/main.rs or src/lib.rs".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustCreateMissingModules)
        && contains_cargo_check(actions)
        && !contains_module_creation(actions, target_root)
    {
        return Err("cargo check planned before creating missing declared module files".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustFixDeadCodeForbidConflict)
        && contains_cargo_check(actions)
        && !contains_dead_code_conflict_fix(actions, target_root)
    {
        return Err("cargo check planned before fixing allow(dead_code) vs forbid(dead_code) conflict".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustFixUnresolvedImport)
        && contains_cargo_check(actions)
        && !contains_expected_hint_target(actions, semantic_summary, "unresolved_import", target_root)
    {
        return Err("cargo check planned before fixing unresolved import".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustDefineMissingSymbol)
        && contains_cargo_check(actions)
        && !contains_expected_hint_target(actions, semantic_summary, "missing_symbol", target_root)
    {
        return Err("cargo check planned before defining or importing missing symbol".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustResolveDuplicateDefinition)
        && contains_cargo_check(actions)
        && !contains_expected_hint_target(actions, semantic_summary, "duplicate_definition", target_root)
    {
        return Err("cargo check planned before resolving duplicate definition".to_string());
    }
    if preconditions.contains(&PlanningPrecondition::MustFixTraitBoundFailure)
        && contains_cargo_check(actions)
        && !contains_expected_hint_target(actions, semantic_summary, "trait_bound_failure", target_root)
    {
        return Err("cargo check planned before fixing actionable trait bound failure".to_string());
    }
    if let Some(highest_priority) = intents.first() {
        validate_highest_priority_intent(actions, target_root, highest_priority, semantic_summary)?;
    }
    Ok(())
}

fn validate_highest_priority_intent(
    actions: &[canon_event::LoopPlanned],
    target_root: &Path,
    intent: &RepairIntent,
    semantic_summary: &SemanticStateSummary,
) -> Result<(), String> {
    let satisfied = match intent {
        RepairIntent::BootstrapWorkspace => contains_bootstrap_action(actions),
        RepairIntent::InitCargoProject => contains_cargo_init(actions),
        RepairIntent::CreateEntrypoint => contains_expected_entrypoint_target(actions, semantic_summary, target_root),
        RepairIntent::CreateMissingModules => contains_expected_module_target(actions, semantic_summary, target_root),
        RepairIntent::FixDeadCodeForbidConflict => contains_expected_dead_code_target(actions, semantic_summary, target_root),
        RepairIntent::FixUnresolvedImport => contains_expected_hint_target(actions, semantic_summary, "unresolved_import", target_root),
        RepairIntent::DefineMissingSymbol => contains_expected_hint_target(actions, semantic_summary, "missing_symbol", target_root),
        RepairIntent::ResolveDuplicateDefinition => contains_expected_hint_target(actions, semantic_summary, "duplicate_definition", target_root),
        RepairIntent::FixTraitBoundFailure => contains_expected_hint_target(actions, semantic_summary, "trait_bound_failure", target_root),
    };
    if satisfied {
        Ok(())
    } else {
        Err(match intent {
            RepairIntent::BootstrapWorkspace => "first planned batch must bootstrap the workspace".to_string(),
            RepairIntent::InitCargoProject => "first planned batch must initialize Cargo in the target directory".to_string(),
            RepairIntent::CreateEntrypoint => "first planned batch must create an entrypoint before validation".to_string(),
            RepairIntent::CreateMissingModules => "first planned batch must create missing declared module files".to_string(),
            RepairIntent::FixDeadCodeForbidConflict => {
                "first planned batch must address the allow(dead_code) vs forbid(dead_code) conflict".to_string()
            }
            RepairIntent::FixUnresolvedImport => "first planned batch must target the unresolved import location".to_string(),
            RepairIntent::DefineMissingSymbol => "first planned batch must target the missing symbol location".to_string(),
            RepairIntent::ResolveDuplicateDefinition => "first planned batch must target the duplicate definition location".to_string(),
            RepairIntent::FixTraitBoundFailure => "first planned batch must target the trait-bound failure location".to_string(),
        })
    }
}

fn contains_expected_entrypoint_target(
    actions: &[canon_event::LoopPlanned],
    semantic_summary: &SemanticStateSummary,
    target_root: &Path,
) -> bool {
    let expected = expected_entrypoint_paths(semantic_summary, target_root);
    if expected.is_empty() {
        contains_entrypoint_creation(actions, target_root)
    } else {
        actions
            .iter()
            .any(|action| touches_any_owned_paths(action, target_root, &expected))
    }
}

fn contains_expected_module_target(
    actions: &[canon_event::LoopPlanned],
    semantic_summary: &SemanticStateSummary,
    target_root: &Path,
) -> bool {
    let expected = expected_module_paths(semantic_summary, target_root);
    if expected.is_empty() {
        contains_module_creation(actions, target_root)
    } else {
        actions
            .iter()
            .any(|action| touches_any_owned_paths(action, target_root, &expected))
    }
}

fn contains_expected_dead_code_target(
    actions: &[canon_event::LoopPlanned],
    semantic_summary: &SemanticStateSummary,
    target_root: &Path,
) -> bool {
    let expected = expected_dead_code_paths(semantic_summary, target_root);
    if expected.is_empty() {
        contains_dead_code_conflict_fix(actions, target_root)
    } else {
        actions.iter().any(|action| {
            touches_any_owned_paths(action, target_root, &expected)
                && (action.action_kind != "apply_patch"
                    || action
                        .action_payload
                        .get("patch")
                        .and_then(|v| v.as_str())
                        .is_some_and(|patch| patch.contains("allow(dead_code)")))
        })
    }
}

fn contains_expected_hint_target(
    actions: &[canon_event::LoopPlanned],
    semantic_summary: &SemanticStateSummary,
    hint_kind: &str,
    target_root: &Path,
) -> bool {
    let expected = expected_hint_paths(semantic_summary, hint_kind, target_root);
    if expected.is_empty() {
        actions.iter().any(|action| !normalized_touched_paths(action, target_root).is_empty())
    } else {
        actions
            .iter()
            .any(|action| touches_any_owned_paths(action, target_root, &expected))
    }
}

fn contains_bootstrap_action(actions: &[canon_event::LoopPlanned]) -> bool {
    actions.iter().any(|action| {
        action.action_kind == "run_command"
            && action
                .action_payload
                .get("cmd")
                .and_then(|v| v.as_str())
                .is_some_and(|cmd| cmd.contains("cargo init") || cmd.contains("cargo new"))
    })
}

fn contains_cargo_init(actions: &[canon_event::LoopPlanned]) -> bool {
    actions.iter().any(|action| {
        action.action_kind == "run_command"
            && action
                .action_payload
                .get("cmd")
                .and_then(|v| v.as_str())
                .is_some_and(|cmd| cmd.contains("cargo init"))
    })
}

fn contains_cargo_check(actions: &[canon_event::LoopPlanned]) -> bool {
    actions.iter().any(|action| {
        action.action_kind == "run_command"
            && action
                .action_payload
                .get("cmd")
                .and_then(|v| v.as_str())
                .is_some_and(|cmd| cmd.contains("cargo check"))
    })
}

fn contains_entrypoint_creation(actions: &[canon_event::LoopPlanned], target_root: &Path) -> bool {
    let main = target_root.join("src/main.rs");
    let lib = target_root.join("src/lib.rs");
    actions
        .iter()
        .any(|action| touches_any_path(action, target_root, &[main.as_path(), lib.as_path()]))
}

fn contains_module_creation(actions: &[canon_event::LoopPlanned], target_root: &Path) -> bool {
    actions.iter().any(|action| {
        touched_paths(action)
            .into_iter()
            .any(|path| path.starts_with(target_root.join("src")) && path.extension().and_then(|s| s.to_str()) == Some("rs"))
    })
}

fn contains_dead_code_conflict_fix(actions: &[canon_event::LoopPlanned], target_root: &Path) -> bool {
    actions.iter().any(|action| {
        if !touches_any_path(
            action,
            target_root,
            &[target_root.join("src/lib.rs").as_path(), target_root.join("src/main.rs").as_path()],
        ) {
            return false;
        }
        if action.action_kind == "apply_patch" {
            return action
                .action_payload
                .get("patch")
                .and_then(|v| v.as_str())
                .is_some_and(|patch| patch.contains("allow(dead_code)"));
        }
        true
    })
}

fn expected_entrypoint_paths(semantic_summary: &SemanticStateSummary, target_root: &Path) -> Vec<PathBuf> {
    match semantic_summary.entrypoint_kind.as_deref().unwrap_or("none") {
        "lib" => vec![target_root.join("src/lib.rs")],
        "bin" => vec![target_root.join("src/main.rs")],
        "mixed" => vec![target_root.join("src/main.rs"), target_root.join("src/lib.rs")],
        _ => vec![target_root.join("src/main.rs"), target_root.join("src/lib.rs")],
    }
}

fn expected_module_paths(semantic_summary: &SemanticStateSummary, target_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for gap in &semantic_summary.module_gaps {
        let Some((_, paths)) = gap.split_once("->") else {
            continue;
        };
        for path in paths.split(" or ") {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(trimmed);
            out.push(if candidate.is_absolute() { candidate } else { target_root.join(trimmed) });
        }
    }
    out
}

fn expected_dead_code_paths(semantic_summary: &SemanticStateSummary, target_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for file in &semantic_summary.source_files {
        if file.ends_with("src/lib.rs") || file.ends_with("src/main.rs") {
            let candidate = PathBuf::from(file);
            out.push(if candidate.is_absolute() { candidate } else { target_root.join(file) });
        }
    }
    if out.is_empty() {
        out.push(target_root.join("src/lib.rs"));
        out.push(target_root.join("src/main.rs"));
    }
    out
}

fn expected_hint_paths(
    semantic_summary: &SemanticStateSummary,
    hint_kind: &str,
    target_root: &Path,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for hint in &semantic_summary.compiler_hints {
        if hint.kind != hint_kind {
            continue;
        }
        for target in &hint.target_files {
            if target == "none" {
                continue;
            }
            let candidate = PathBuf::from(target);
            out.push(if candidate.is_absolute() {
                candidate
            } else {
                target_root.join(target)
            });
        }
    }
    out.sort();
    out.dedup();
    out
}

fn touches_any_path(action: &canon_event::LoopPlanned, target_root: &Path, expected: &[&Path]) -> bool {
    let touched = normalized_touched_paths(action, target_root);
    expected.iter().any(|path| touched.iter().any(|candidate| candidate == path))
}

fn touches_any_owned_paths(action: &canon_event::LoopPlanned, target_root: &Path, expected: &[PathBuf]) -> bool {
    let touched = normalized_touched_paths(action, target_root);
    expected.iter().any(|path| touched.iter().any(|candidate| candidate == path))
}

fn normalized_touched_paths(action: &canon_event::LoopPlanned, target_root: &Path) -> Vec<PathBuf> {
    touched_paths(action)
        .into_iter()
        .map(|path| if path.is_absolute() { path } else { target_root.join(path) })
        .collect()
}

fn touched_paths(action: &canon_event::LoopPlanned) -> Vec<PathBuf> {
    match action.action_kind.as_str() {
        "write_file" | "read_file" | "list_dir" | "patch_file" => action
            .action_payload
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .into_iter()
            .collect(),
        "apply_patch" => action
            .action_payload
            .get("patch")
            .and_then(|v| v.as_str())
            .and_then(|patch| parse_patch(patch).ok())
            .map(|args| {
                args.hunks
                    .into_iter()
                    .map(|hunk| match hunk {
                        canon_tools_patch::Hunk::AddFile { path, .. }
                        | canon_tools_patch::Hunk::DeleteFile { path }
                        | canon_tools_patch::Hunk::UpdateFile { path, .. } => path,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{derive_preconditions, validate_preconditions, PlanningPrecondition};
    use crate::env_model::{EntrypointKind, WorkspaceModel};
    use canon_semantic_state::SemanticStateSummary;
    use std::path::Path;

    #[test]
    fn derives_workspace_preconditions() {
        let model = WorkspaceModel {
            target_root: "/tmp/example".into(),
            path_exists: true,
            repo_initialized: false,
            cargo_toml_exists: false,
            cargo_lock_exists: false,
            crate_name: None,
            src_dir_exists: false,
            entrypoint_kind: EntrypointKind::None,
            rust_file_count: 0,
            source_files: Vec::new(),
            module_gaps: Vec::new(),
        };
        let preconditions = derive_preconditions(Some(&model), &[]);
        assert!(preconditions.contains(&PlanningPrecondition::MustInitCargoProject));
    }

    #[test]
    fn rejects_cargo_check_before_entrypoint_creation() {
        let actions = vec![canon_event::LoopPlanned {
            tick: 0,
            action_kind: "run_command".to_string(),
            action_payload: serde_json::json!({"cmd":"cargo check","cwd":"/tmp/example"}),
            reason: String::new(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            signals: None,
            depends_on: Vec::new(),
        }];
        let result = validate_preconditions(
            &actions,
            Path::new("/tmp/example"),
            &[PlanningPrecondition::MustCreateEntrypoint],
            &SemanticStateSummary::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_missing_module_repair_that_targets_the_wrong_file() {
        let actions = vec![canon_event::LoopPlanned {
            tick: 0,
            action_kind: "apply_patch".to_string(),
            action_payload: serde_json::json!({
                "patch": "*** Begin Patch\n*** Add File: src/other.rs\n+pub struct Other;\n*** End Patch\n"
            }),
            reason: String::new(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            signals: None,
            depends_on: Vec::new(),
        }];
        let summary = SemanticStateSummary {
            complete: true,
            target_root: Some("/tmp/example".into()),
            module_gaps: vec!["index -> src/index.rs".into()],
            ..SemanticStateSummary::default()
        };
        let result = validate_preconditions(
            &actions,
            Path::new("/tmp/example"),
            &[PlanningPrecondition::MustCreateMissingModules],
            &summary,
        );
        assert!(result.is_err());
    }

    #[test]
    fn accepts_missing_module_repair_that_targets_expected_path() {
        let actions = vec![canon_event::LoopPlanned {
            tick: 0,
            action_kind: "apply_patch".to_string(),
            action_payload: serde_json::json!({
                "patch": "*** Begin Patch\n*** Add File: src/index.rs\n+pub struct Index;\n*** End Patch\n"
            }),
            reason: String::new(),
            llm_request_id: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            signals: None,
            depends_on: Vec::new(),
        }];
        let summary = SemanticStateSummary {
            complete: true,
            target_root: Some("/tmp/example".into()),
            module_gaps: vec!["index -> src/index.rs".into()],
            ..SemanticStateSummary::default()
        };
        let result = validate_preconditions(
            &actions,
            Path::new("/tmp/example"),
            &[PlanningPrecondition::MustCreateMissingModules],
            &summary,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn repair_intents_preserve_priority_order() {
        let intents = super::derive_repair_intents(&[
            PlanningPrecondition::MustBootstrapWorkspace,
            PlanningPrecondition::MustCreateMissingModules,
        ]);
        assert_eq!(
            intents,
            vec![
                super::RepairIntent::BootstrapWorkspace,
                super::RepairIntent::CreateMissingModules,
            ]
        );
    }

    #[test]
    fn derive_preconditions_from_lines_round_trips() {
        let derived = super::derive_preconditions_from_lines(&[
            "must_create_entrypoint=true repair=create_src_main_or_lib_before_cargo_check".into(),
            "must_fix_dead_code_forbid_conflict=true repair=remove_allow_dead_code_or_make_code_used".into(),
            "must_fix_unresolved_import=true repair=edit_import_or_define_missing_import_target".into(),
        ]);
        assert_eq!(
            derived,
            vec![
                PlanningPrecondition::MustCreateEntrypoint,
                PlanningPrecondition::MustFixDeadCodeForbidConflict,
                PlanningPrecondition::MustFixUnresolvedImport,
            ]
        );
    }
}
