pub mod api;
pub mod check;
pub mod consumer;
pub mod edit;
pub mod fs;
pub mod git;
pub mod query;
pub mod structured;
pub mod symbol_index;
pub mod tlog;
pub mod verify;

use std::path::Path;
use std::sync::Arc;
use std::fs as stdfs;

use anyhow::anyhow;

use crate::symbol_index::SymbolIndex;
use crate::tlog::publish_invariant_error;
pub use consumer::EditConsumer;
use edit::ProjectEditor;
use structured::{EditOp, FieldMutation};
use verify::verify_renames_applied;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RenameRunReport {
    pub rustc_args: Vec<String>,
    pub def_paths: Vec<String>,
    pub error: Option<String>,
}

impl RenameRunReport {
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "error"
        } else {
            "ok"
        }
    }
}

pub fn rename_symbol_pairs(project: &Path, renames: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let session = match SymbolIndex::build(project) {
        Ok(session) => Arc::new(session),
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };
    rename_symbol_pairs_with_session(project, session, renames)
}

pub fn rename_symbol_pairs_from_graph_candidates(
    project: &Path,
    candidates: &[canon_analysis::GraphRenameCandidate],
) -> RenameRunReport {
    let renames: Vec<(String, String)> = candidates
        .iter()
        .map(|candidate| (candidate.symbol_path.clone(), candidate.suggested_path.clone()))
        .collect();
    rename_symbol_pairs(project, &renames)
}

pub fn rename_duplicate_symbols_from_latest_graph(
    project: &Path,
    limit: usize,
) -> RenameRunReport {
    match canon_analysis::graph_backed_rename_candidates(project, limit) {
        Ok(candidates) => rename_symbol_pairs_from_graph_candidates(project, &candidates),
        Err(err) => RenameRunReport {
            error: Some(format!("{err:?}")),
            ..RenameRunReport::default()
        },
    }
}

pub fn move_symbol_pairs(project: &Path, moves: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let session = match SymbolIndex::build(project) {
        Ok(session) => Arc::new(session),
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };
    let mut editor = match ProjectEditor::load_with_session(project, session) {
        Ok(editor) => editor,
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };

    for (symbol_id, new_module_path) in moves {
        let handle = match editor.synthetic_handle_from_symbol_id(symbol_id) {
            Ok(handle) => handle,
            Err(err) => {
                report.error = Some(format!("{err:?}"));
                return report;
            }
        };
        let op = EditOp::MoveSymbol {
            handle,
            symbol_id: symbol_id.clone(),
            new_module_path: new_module_path.clone(),
            new_crate: None,
        };
        if let Err(err) = editor.queue(symbol_id, op) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    let preview_output = editor
        .validate()
        .and_then(|conflicts| if conflicts.is_empty() { Ok(()) } else { Err(anyhow!("validation conflicts: {conflicts:?}")) })
        .and_then(|_| editor.apply().and_then(|report| if report.conflicts.is_empty() { Ok(()) } else { Err(anyhow!("apply conflicts: {:?}", report.conflicts)) }))
        .and_then(|_| editor.preview());

    match preview_output {
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        Ok(preview) => report.def_paths.push(preview),
    }

    if let Err(err) = editor.commit() {
        report.error = Some(format!("{err:?}"));
    }

    report
}

pub fn move_symbols_from_graph_candidates(
    project: &Path,
    candidates: &[canon_analysis::GraphModuleMoveCandidate],
) -> RenameRunReport {
    let moves: Vec<(String, String)> = candidates
        .iter()
        .map(|candidate| (candidate.symbol_path.clone(), candidate.to_module_path.clone()))
        .collect();
    move_symbol_pairs(project, &moves)
}

pub fn restructure_modules_from_latest_graph(project: &Path, limit: usize) -> RenameRunReport {
    match canon_analysis::graph_backed_module_moves(project, limit) {
        Ok(candidates) => move_symbols_from_graph_candidates(project, &candidates),
        Err(err) => RenameRunReport {
            error: Some(format!("{err:?}")),
            ..RenameRunReport::default()
        },
    }
}

pub fn add_import_paths(project: &Path, imports: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    for (path, import_path) in imports {
        let full_path = project.join(path);
        if let Err(err) = add_import_path(&full_path, import_path) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        report.def_paths.push(path.clone());
    }
    report
}

pub fn define_symbol_stubs(
    project: &Path,
    stubs: &[(String, String, String)],
) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    for (path, symbol, kind) in stubs {
        let full_path = project.join(path);
        if let Err(err) = define_symbol_stub(&full_path, symbol, kind) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        report.def_paths.push(path.clone());
    }
    report
}

pub fn create_module_files(
    project: &Path,
    modules: &[(String, Option<String>)],
) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    for (path, module_name) in modules {
        let full_path = project.join(path);
        if let Err(err) = create_module_file(&full_path, module_name.as_deref()) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        report.def_paths.push(path.clone());
    }
    report
}

pub fn rename_symbol_pairs_with_session(project: &Path, session: Arc<SymbolIndex>, renames: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    if let Err(err) = session.validate_invariants() {
        publish_invariant_error(
            project,
            "rename_symbol_index",
            &err.to_string(),
            serde_json::json!({ "renames": renames }),
        );
        report.error = Some(err.to_string());
        return report;
    }
    let mut editor = match ProjectEditor::load_with_session(project, session) {
        Ok(editor) => editor,
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };

    for (old_symbol, new_symbol) in renames {
        if !editor
            .session
            .as_ref()
            .is_some_and(|index| index.contains(old_symbol))
        {
            let message = format!("preflight invariant: symbol not in index: {old_symbol}");
            publish_invariant_error(
                project,
                "rename_symbol_preflight",
                &message,
                serde_json::json!({ "old_symbol": old_symbol, "new_symbol": new_symbol }),
            );
            report.error = Some(message);
            return report;
        }
        if old_symbol != new_symbol
            && editor
                .session
                .as_ref()
                .is_some_and(|index| index.contains(new_symbol))
        {
            let message = format!("preflight invariant: target symbol already exists: {new_symbol}");
            publish_invariant_error(
                project,
                "rename_symbol_preflight",
                &message,
                serde_json::json!({ "old_symbol": old_symbol, "new_symbol": new_symbol }),
            );
            report.error = Some(message);
            return report;
        }
        let Some(handle) = editor.registry.handles.get(old_symbol).cloned() else {
            let message = format!("preflight invariant: symbol not found in registry: {old_symbol}");
            publish_invariant_error(
                project,
                "rename_symbol_preflight",
                &message,
                serde_json::json!({ "old_symbol": old_symbol, "new_symbol": new_symbol }),
            );
            report.error = Some(message);
            return report;
        };
        let new_ident = new_symbol.rsplit("::").next().unwrap_or(new_symbol.as_str());
        let op = EditOp::MutateField { handle, symbol_id: old_symbol.to_string(), mutation: FieldMutation::RenameIdent(new_ident.to_string()) };
        if let Err(err) = editor.queue(old_symbol, op) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    let preview_output = editor
        .validate()
        .and_then(|conflicts| if conflicts.is_empty() { Ok(()) } else { Err(anyhow!("validation conflicts: {conflicts:?}")) })
        .and_then(|_| editor.apply().and_then(|report| if report.conflicts.is_empty() { Ok(()) } else { Err(anyhow!("apply conflicts: {:?}", report.conflicts)) }))
        .and_then(|_| editor.preview());

    match preview_output {
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        Ok(preview) => report.def_paths.push(preview),
    }

    if let Err(err) = editor.commit() {
        report.error = Some(format!("{err:?}"));
        return report;
    }

    let verify = verify_renames_applied(
        editor
            .session
            .as_ref()
            .expect("editor session must exist for rename verification"),
        &editor,
        renames,
    );
    if verify.pairs_checked != renames.len() || verify.pairs_changed != renames.len() {
        let message = format!(
            "post invariant: rename verification mismatch checked={} changed={} expected={}",
            verify.pairs_checked,
            verify.pairs_changed,
            renames.len()
        );
        publish_invariant_error(
            project,
            "rename_symbol_post",
            &message,
            serde_json::json!({
                "pairs_checked": verify.pairs_checked,
                "pairs_changed": verify.pairs_changed,
                "expected": renames.len(),
            }),
        );
        report.error = Some(message);
    }

    report
}

fn add_import_path(file_path: &Path, import_path: &str) -> anyhow::Result<()> {
    let mut content = stdfs::read_to_string(file_path)?;
    let import_line = format!("use {import_path};");
    if content.lines().any(|line| line.trim() == import_line) {
        return Ok(());
    }
    let parsed = syn::parse_str::<syn::ItemUse>(&import_line)?;
    let rendered = prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![syn::Item::Use(parsed)],
    });
    let insert_at = import_insertion_offset(&content);
    content.insert_str(insert_at, &(rendered + "\n"));
    stdfs::write(file_path, content)?;
    Ok(())
}

fn define_symbol_stub(file_path: &Path, symbol: &str, kind: &str) -> anyhow::Result<()> {
    let mut content = stdfs::read_to_string(file_path)?;
    if content.contains(&format!("fn {symbol}"))
        || content.contains(&format!("struct {symbol}"))
        || content.contains(&format!("enum {symbol}"))
        || content.contains(&format!("trait {symbol}"))
        || content.contains(&format!("type {symbol}"))
        || content.contains(&format!("const {symbol}"))
    {
        return Ok(());
    }
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&stub_for_symbol(symbol, kind));
    if !content.ends_with('\n') {
        content.push('\n');
    }
    stdfs::write(file_path, content)?;
    Ok(())
}

fn create_module_file(file_path: &Path, module_name: Option<&str>) -> anyhow::Result<()> {
    if file_path.exists() {
        return Ok(());
    }
    if let Some(parent) = file_path.parent() {
        stdfs::create_dir_all(parent)?;
    }
    let module_name = module_name
        .or_else(|| file_path.file_stem().and_then(|stem| stem.to_str()))
        .unwrap_or("module");
    let content = format!("// module: {module_name}\n");
    stdfs::write(file_path, content)?;
    Ok(())
}

fn import_insertion_offset(content: &str) -> usize {
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("#!") || trimmed.is_empty() {
            offset += line.len();
            continue;
        }
        break;
    }
    offset
}

fn stub_for_symbol(symbol: &str, kind: &str) -> String {
    match kind {
        "struct" => format!("pub struct {symbol};\n"),
        "enum" => format!("pub enum {symbol} {{\n    Todo,\n}}\n"),
        "trait" => format!("pub trait {symbol} {{}}\n"),
        "type" => format!("pub type {symbol} = ();\n"),
        "const" => format!("pub const {symbol}: () = ();\n"),
        _ => format!("pub fn {symbol}() {{\n    todo!()\n}}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::{add_import_paths, create_module_files, define_symbol_stubs};
    use std::fs;

    #[test]
    fn add_import_paths_inserts_use_line() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("lib.rs");
        fs::write(&file, "#![allow(unused)]\n\npub fn run() {}\n").unwrap();
        let report = add_import_paths(dir.path(), &[("src/lib.rs".into(), "crate::cli::Cli".into())]);
        assert!(report.error.is_none());
        let updated = fs::read_to_string(file).unwrap();
        assert!(updated.contains("use crate::cli::Cli;"));
    }

    #[test]
    fn define_symbol_stubs_appends_function_stub() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("lib.rs");
        fs::write(&file, "pub fn existing() {}\n").unwrap();
        let report = define_symbol_stubs(
            dir.path(),
            &[("src/lib.rs".into(), "run".into(), "fn".into())],
        );
        assert!(report.error.is_none());
        let updated = fs::read_to_string(file).unwrap();
        assert!(updated.contains("pub fn run()"));
    }

    #[test]
    fn create_module_files_writes_missing_module() {
        let dir = tempfile::tempdir().unwrap();
        let report = create_module_files(
            dir.path(),
            &[("src/merge.rs".into(), Some("merge".into()))],
        );
        assert!(report.error.is_none());
        let created = fs::read_to_string(dir.path().join("src/merge.rs")).unwrap();
        assert!(created.contains("module: merge"));
    }
}

#[cfg(test)]
mod capability_suite;
