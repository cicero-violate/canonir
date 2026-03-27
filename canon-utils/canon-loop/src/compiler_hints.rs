#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerHint {
    pub kind: CompilerHintKind,
    pub summary: String,
    pub suggested_repair: String,
    pub target_files: Vec<String>,
}

pub fn extract_compiler_hints(errors: &[serde_json::Value]) -> Vec<CompilerHint> {
    let mut hints = Vec::new();
    for text in extract_error_texts(errors) {
        let target_files = extract_target_files(&text);
        if let Some(module_name) = extract_missing_module_name(&text) {
            hints.push(CompilerHint {
                kind: CompilerHintKind::MissingModule,
                summary: format!("compiler reports missing module `{module_name}`"),
                suggested_repair: format!(
                    "create the missing module file for `{module_name}` and wire it before cargo check"
                ),
                target_files,
            });
            continue;
        }
        if text.contains("allow(dead_code) incompatible with previous forbid") {
            hints.push(CompilerHint {
                kind: CompilerHintKind::DeadCodeForbidConflict,
                summary: "compiler forbids dead_code while source adds allow(dead_code)".to_string(),
                suggested_repair: "remove allow(dead_code) or make the code used; do not suppress this lint".to_string(),
                target_files,
            });
            continue;
        }
        if text.contains("main function not found") || text.contains("`main` function not found") {
            hints.push(CompilerHint {
                kind: CompilerHintKind::MissingEntrypoint,
                summary: "compiler reports missing main entrypoint".to_string(),
                suggested_repair: "create src/main.rs with a valid main function or convert the crate to a library".to_string(),
                target_files,
            });
            continue;
        }
        if let Some(symbol) = extract_unresolved_import_symbol(&text) {
            hints.push(CompilerHint {
                kind: CompilerHintKind::UnresolvedImport,
                summary: format!("compiler reports unresolved import `{symbol}`"),
                suggested_repair: "add the missing import target or correct the import path before cargo check".to_string(),
                target_files,
            });
            continue;
        }
        if let Some(symbol) = extract_missing_symbol(&text) {
            hints.push(CompilerHint {
                kind: CompilerHintKind::MissingSymbol,
                summary: format!("compiler cannot find `{symbol}` in scope"),
                suggested_repair: "define the missing symbol or import it before cargo check".to_string(),
                target_files,
            });
            continue;
        }
        if let Some(symbol) = extract_duplicate_definition_symbol(&text) {
            hints.push(CompilerHint {
                kind: CompilerHintKind::DuplicateDefinition,
                summary: format!("compiler reports duplicate definition for `{symbol}`"),
                suggested_repair: "remove or rename the duplicate definition before cargo check".to_string(),
                target_files,
            });
            continue;
        }
        if let Some(bound) = extract_trait_bound_summary(&text) {
            hints.push(CompilerHint {
                kind: CompilerHintKind::TraitBoundFailure,
                summary: format!("compiler reports unsatisfied trait bound `{bound}`"),
                suggested_repair: "edit the local type, impl, or call site to satisfy the required trait bound".to_string(),
                target_files,
            });
            continue;
        }
        if text.contains("error[E") || text.contains("could not compile") {
            hints.push(CompilerHint {
                kind: CompilerHintKind::GenericCompilerFailure,
                summary: truncate(&text, 140),
                suggested_repair: "address the cited compiler error directly before adding more edits".to_string(),
                target_files,
            });
        }
    }
    dedup_hints(hints)
}

pub fn planner_lines(errors: &[serde_json::Value]) -> Vec<String> {
    extract_compiler_hints(errors)
        .into_iter()
        .map(|hint| {
            let targets = if hint.target_files.is_empty() {
                "none".to_string()
            } else {
                hint.target_files.join("|")
            };
            format!(
                "kind={} targets={} summary={} repair={}",
                hint.kind.as_str(),
                targets,
                hint.summary,
                hint.suggested_repair
            )
        })
        .collect()
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
        if let Some(message) = value
            .get("message")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
        {
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
    extract_backticked_after(text, "unresolved import `")
        .or_else(|| extract_backticked_after(text, "no `"))
}

fn extract_missing_symbol(text: &str) -> Option<String> {
    extract_backticked_after(text, "cannot find function `")
        .or_else(|| extract_backticked_after(text, "cannot find type `"))
        .or_else(|| extract_backticked_after(text, "cannot find value `"))
        .or_else(|| extract_backticked_after(text, "cannot find struct, variant or union type `"))
}

fn extract_duplicate_definition_symbol(text: &str) -> Option<String> {
    extract_backticked_after(text, "the name `")
        .or_else(|| extract_backticked_after(text, "duplicate definitions with name `"))
}

fn extract_trait_bound_summary(text: &str) -> Option<String> {
    let marker = "the trait bound `";
    let start = text.find(marker)? + marker.len();
    let tail = &text[start..];
    let end = tail.find('`')?;
    let bound = tail[..end].trim();
    if bound.is_empty() { None } else { Some(bound.to_string()) }
}

fn extract_backticked_after(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let tail = &text[start..];
    let end = tail.find('`')?;
    let value = tail[..end].trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
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

fn dedup_hints(hints: Vec<CompilerHint>) -> Vec<CompilerHint> {
    let mut out = Vec::new();
    for hint in hints {
        if !out.iter().any(|existing: &CompilerHint| {
            existing.kind == hint.kind
                && existing.summary == hint.summary
                && existing.target_files == hint.target_files
        }) {
            out.push(hint);
        }
    }
    out
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
    use super::{extract_compiler_hints, planner_lines, CompilerHintKind};

    #[test]
    fn extracts_missing_module_hint() {
        let errors = vec![serde_json::json!({
            "message": {"message": "error[E0583]: file not found for module `index`"}
        })];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind == CompilerHintKind::MissingModule));
    }

    #[test]
    fn extracts_dead_code_forbid_hint() {
        let errors = vec![serde_json::json!("error[E0453]: allow(dead_code) incompatible with previous forbid")];
        let lines = planner_lines(&errors);
        assert!(lines.iter().any(|line| line.contains("remove allow(dead_code)")));
    }

    #[test]
    fn extracts_unresolved_import_hint() {
        let errors = vec![serde_json::json!("error[E0432]: unresolved import `crate::foo`\n --> src/lib.rs:1:5")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind == CompilerHintKind::UnresolvedImport));
    }

    #[test]
    fn extracts_missing_symbol_hint() {
        let errors = vec![serde_json::json!("error[E0425]: cannot find function `run` in this scope\n --> src/main.rs:3:5")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind == CompilerHintKind::MissingSymbol));
    }

    #[test]
    fn extracts_duplicate_definition_hint() {
        let errors = vec![serde_json::json!("error[E0255]: the name `Engine` is defined multiple times\n --> src/lib.rs:7:1")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind == CompilerHintKind::DuplicateDefinition));
    }

    #[test]
    fn extracts_trait_bound_hint() {
        let errors = vec![serde_json::json!("error[E0277]: the trait bound `Foo: Clone` is not satisfied\n --> src/lib.rs:8:10")];
        let hints = extract_compiler_hints(&errors);
        assert!(hints.iter().any(|h| h.kind == CompilerHintKind::TraitBoundFailure));
    }
}
