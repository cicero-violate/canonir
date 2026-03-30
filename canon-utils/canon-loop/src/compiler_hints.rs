use canon_semantic_state::{CompilerHintKind, CompilerHintRecord, FailureClassKind, FailureScopeKind};

pub fn extract_compiler_hints(errors: &[serde_json::Value]) -> Vec<CompilerHintRecord> {
    let mut hints = Vec::new();
    for text in extract_error_texts(errors) {
        let target_files = extract_target_files(&text);
        let scope = classify_failure_scope(&text, &target_files);
        if let Some(module_name) = extract_missing_module_name(&text) {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::MissingModule,
                    format!("compiler reports missing module `{module_name}`"),
                    format!("use semantic module creation to add `{module_name}` before cargo check"),
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if text.contains("allow(dead_code) incompatible with previous forbid") {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::DeadCodeForbidConflict,
                    "compiler forbids dead_code while source adds allow(dead_code)",
                    "remove allow(dead_code) or make the code used; do not suppress this lint",
                    target_files,
                )
                .with_failure_scope(FailureScopeKind::Workspace),
            );
            continue;
        }
        if text.contains("main function not found") || text.contains("`main` function not found") {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::MissingEntrypoint,
                    "compiler reports missing main entrypoint",
                    "create src/main.rs with a valid main function or convert the crate to a library",
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if let Some(symbol) = extract_unresolved_import_symbol(&text) {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::UnresolvedImport,
                    format!("compiler reports unresolved import `{symbol}`"),
                    "use semantic import repair to add or correct the import before cargo check",
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if let Some(symbol) = extract_missing_symbol(&text) {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::MissingSymbol,
                    format!("compiler cannot find `{symbol}` in scope"),
                    "use semantic symbol definition or import repair before cargo check",
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if let Some(symbol) = extract_duplicate_definition_symbol(&text) {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::DuplicateDefinition,
                    format!("compiler reports duplicate definition for `{symbol}`"),
                    "use semantic rename to resolve the duplicate definition before cargo check",
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if let Some(bound) = extract_trait_bound_summary(&text) {
            hints.push(
                CompilerHintRecord::new(
                    CompilerHintKind::TraitBoundFailure,
                    format!("compiler reports unsatisfied trait bound `{bound}`"),
                    "edit the local type, impl, or call site to satisfy the required trait bound",
                    target_files,
                )
                .with_failure_scope(scope),
            );
            continue;
        }
        if text.contains("target path does not exist") || text.contains("could not find `Cargo.toml`") || text.contains("failed to load manifest") || text.contains("manifest path") {
            hints.push(
                CompilerHintRecord::new(CompilerHintKind::GenericCompilerFailure, truncate(&text, 140), "repair or refresh the workspace/bootstrap state before cargo check", target_files)
                    .with_failure_scope(FailureScopeKind::Workspace),
            );
            continue;
        }
        if text.contains("error[E") || text.contains("could not compile") {
            hints.push(
                CompilerHintRecord::new(CompilerHintKind::GenericCompilerFailure, truncate(&text, 140), "address the cited compiler error directly before adding more edits", target_files)
                    .with_failure_scope(scope),
            );
        }
    }
    dedup_hints(hints)
}

pub fn classify_failure_scope(text: &str, target_files: &[String]) -> FailureScopeKind {
    if text.contains("rustc capture failed") {
        return FailureScopeKind::Tooling;
    }
    if target_files.iter().any(|target| !target.trim().is_empty() && target != "none") {
        return FailureScopeKind::Localized;
    }
    if text.contains(".cargo/config.toml")
        || text.contains("Cargo.toml")
        || text.contains("workspace")
        || text.contains("forbid")
        || text.contains("target path does not exist")
        || text.contains("failed to load manifest")
        || text.contains("manifest path")
        || text.contains("cannot be run on existing Cargo packages")
        || text.contains("use `cargo new`")
        || text.contains("No such file or directory")
    {
        return FailureScopeKind::Workspace;
    }
    if text.contains("cargo ") || text.contains("process didn't exit successfully") {
        return FailureScopeKind::Tooling;
    }
    FailureScopeKind::Tooling
}

pub fn classify_failure_metadata(text: &str) -> (FailureClassKind, FailureScopeKind) {
    let target_files = extract_target_files(text);
    (classify_failure_class(text), classify_failure_scope(text, &target_files))
}

pub fn classify_failure_class(text: &str) -> FailureClassKind {
    if extract_missing_module_name(text).is_some() {
        FailureClassKind::MissingModule
    } else if text.contains("allow(dead_code) incompatible with previous forbid") {
        FailureClassKind::DeadCodeForbidConflict
    } else if text.contains("main function not found") || text.contains("`main` function not found") {
        FailureClassKind::MissingEntrypoint
    } else if extract_unresolved_import_symbol(text).is_some() {
        FailureClassKind::UnresolvedImport
    } else if extract_missing_symbol(text).is_some() {
        FailureClassKind::MissingSymbol
    } else if extract_duplicate_definition_symbol(text).is_some() {
        FailureClassKind::DuplicateDefinition
    } else if extract_trait_bound_summary(text).is_some() {
        FailureClassKind::TraitBoundFailure
    } else {
        FailureClassKind::GenericCompilerFailure
    }
}

pub fn planner_lines(errors: &[serde_json::Value]) -> Vec<CompilerHintRecord> {
    extract_compiler_hints(errors)
}

fn extract_error_texts(errors: &[serde_json::Value]) -> Vec<String> {
    let mut out = Vec::new();
    for value in errors {
        if let Some(text) = value.as_str() {
            if !text.trim().is_empty() {
                out.push(text.trim().to_string());
            }
            continue;
        }
        if let Some(message) = value.get("message").and_then(|v| v.get("message")).and_then(|v| v.as_str()) {
            if !message.trim().is_empty() {
                out.push(message.trim().to_string());
            }
        }
        if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
            if !message.trim().is_empty() {
                out.push(message.trim().to_string());
            }
        }
    }
    out
}

fn extract_missing_module_name(text: &str) -> Option<String> {
    let marker = "file not found for module `";
    let start = text.find(marker)? + marker.len();
    let tail = &text[start..];
    let end = tail.find('`')?;
    let module = tail[..end].trim();
    if module.is_empty() {
        None
    } else {
        Some(module.to_string())
    }
}

fn extract_unresolved_import_symbol(text: &str) -> Option<String> {
    extract_backticked_after(text, "unresolved import `").or_else(|| extract_backticked_after(text, "no `"))
}

fn extract_missing_symbol(text: &str) -> Option<String> {
    extract_backticked_after(text, "cannot find function `")
        .or_else(|| extract_backticked_after(text, "cannot find type `"))
        .or_else(|| extract_backticked_after(text, "cannot find value `"))
        .or_else(|| extract_backticked_after(text, "cannot find struct, variant or union type `"))
}

fn extract_duplicate_definition_symbol(text: &str) -> Option<String> {
    extract_backticked_after(text, "the name `").or_else(|| extract_backticked_after(text, "duplicate definitions with name `"))
}

fn extract_trait_bound_summary(text: &str) -> Option<String> {
    let marker = "the trait bound `";
    let start = text.find(marker)? + marker.len();
    let tail = &text[start..];
    let end = tail.find('`')?;
    let bound = tail[..end].trim();
    if bound.is_empty() {
        None
    } else {
        Some(bound.to_string())
    }
}

fn extract_backticked_after(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let tail = &text[start..];
    let end = tail.find('`')?;
    let value = tail[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_target_files(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("--> ") {
            let path = rest.split(':').next().unwrap_or("").trim();
            if !path.is_empty() {
                out.push(path.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn dedup_hints(hints: Vec<CompilerHintRecord>) -> Vec<CompilerHintRecord> {
    let mut out: Vec<CompilerHintRecord> = Vec::new();
    for hint in hints {
        if !out.iter().any(|existing| existing.kind == hint.kind && existing.summary == hint.summary && existing.target_files == hint.target_files) {
            out.push(hint);
        }
    }
    out
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_compiler_hints, planner_lines};
    use canon_semantic_state::{CompilerHintKind, FailureScopeKind};

    #[test]
    fn extracts_missing_module_hint() {
        let errors = vec![serde_json::json!({
            "message": {"message": "error[E0583]: file not found for module `index`"}
        })];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind_enum() == Some(CompilerHintKind::MissingModule)));
    }

    #[test]
    fn extracts_dead_code_forbid_hint() {
        let errors = vec![serde_json::json!("error[E0453]: allow(dead_code) incompatible with previous forbid")];
        let lines = planner_lines(&errors);
        assert!(lines.iter().any(|line| line.suggested_repair.contains("remove allow(dead_code)")));
        assert!(lines.iter().any(|line| line.failure_scope_enum() == Some(FailureScopeKind::Workspace)));
    }

    #[test]
    fn extracts_unresolved_import_hint() {
        let errors = vec![serde_json::json!("error[E0432]: unresolved import `crate::foo`\n --> src/lib.rs:1:5")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind_enum() == Some(CompilerHintKind::UnresolvedImport)));
        assert!(hints.iter().any(|h| h.failure_scope_enum() == Some(FailureScopeKind::Localized)));
    }

    #[test]
    fn extracts_missing_symbol_hint() {
        let errors = vec![serde_json::json!("error[E0425]: cannot find function `run` in this scope\n --> src/main.rs:3:5")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind_enum() == Some(CompilerHintKind::MissingSymbol)));
    }

    #[test]
    fn extracts_duplicate_definition_hint() {
        let errors = vec![serde_json::json!("error[E0255]: the name `Engine` is defined multiple times\n --> src/lib.rs:7:1")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind_enum() == Some(CompilerHintKind::DuplicateDefinition)));
    }

    #[test]
    fn extracts_trait_bound_hint() {
        let errors = vec![serde_json::json!("error[E0277]: the trait bound `Foo: Clone` is not satisfied\n --> src/lib.rs:8:10")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind_enum() == Some(CompilerHintKind::TraitBoundFailure)));
    }
}
