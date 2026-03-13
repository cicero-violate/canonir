use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::edit::ProjectEditor;
use crate::structured::{EditOp, FieldMutation};
use crate::tlog::publish_edit_event;

#[derive(Deserialize)]
pub struct ApiRequest {
    pub project: String,
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub check: bool,
    pub ops: Vec<ApiOp>,
}

#[derive(Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum ApiOp {
    RenameSymbol { old: String, new: String },
    MoveSymbol { symbol_id: String, new_module_path: String },
    RenameModule { old_module_path: String, new_name: String },
    RenameDir { old_dir: String, new_dir: String },
    Help,
    ListOps,
    ListSymbols,
    GetSymbol { symbol_id: String },
    PreviewRename,
    CheckErrors,
    DeleteSymbol { symbol_id: String },
    InlineModule { module_path: String },
    ExtractModule { symbol_id: String, target_file: String },
    SuggestRenames { batch_size: Option<usize>, model: Option<String> },
    ApplySuggestions { suggestions: Vec<(String, String)> },
}

#[derive(Serialize)]
pub struct ApiResult {
    pub op: String,
    pub status: String,
    pub detail: Option<String>,
    pub data: Option<serde_json::Value>,
}

fn api_result(op: &str, status: &str, detail: Option<String>, data: Option<serde_json::Value>) -> ApiResult {
    ApiResult { op: op.into(), status: status.into(), detail, data }
}

fn api_error(op: &str, err: impl ToString) -> ApiResult {
    api_result(op, "error", Some(err.to_string()), None)
}

fn api_ok(op: &str, data: Option<serde_json::Value>) -> ApiResult {
    api_result(op, "ok", None, data)
}

fn api_queued(op: &str) -> ApiResult {
    api_result(op, "queued", None, None)
}

fn api_unsupported(op: &str, detail: &str) -> ApiResult {
    api_result(op, "unsupported", Some(detail.to_string()), None)
}

fn publish_result(op: &str, res: Result<(), anyhow::Error>) -> ApiResult {
    match res {
        Ok(_) => api_queued(op),
        Err(e) => api_error(op, e),
    }
}

fn result_or_error<T, E: ToString>(op: &str, res: Result<T, E>) -> Result<T, ApiResult> {
    res.map_err(|e| api_error(op, e))
}

fn handle_rename_symbol(editor: &mut ProjectEditor, old: &str, new: &str) -> ApiResult {
    let new_ident = match result_or_error("RenameSymbol", normalize_new_ident(new)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match result_or_error("RenameSymbol", editor.synthetic_handle_from_symbol_id(old)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let op = EditOp::MutateField { handle, symbol_id: old.to_string(), mutation: FieldMutation::RenameIdent(new_ident) };
    publish_result(
        "RenameSymbol",
        publish_edit_event(&editor.project_root, op.to_event(editor.project_root.to_string_lossy().as_ref())),
    )
}

fn handle_move_symbol(editor: &mut ProjectEditor, symbol_id: &str, new_module_path: &str) -> ApiResult {
    let handle = match result_or_error("MoveSymbol", editor.synthetic_handle_from_symbol_id(symbol_id)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let op = EditOp::MoveSymbol { handle, symbol_id: symbol_id.to_string(), new_module_path: new_module_path.to_string(), new_crate: None };
    publish_result(
        "MoveSymbol",
        publish_edit_event(&editor.project_root, op.to_event(editor.project_root.to_string_lossy().as_ref())),
    )
}

fn handle_delete_symbol(editor: &mut ProjectEditor, symbol_id: &str) -> ApiResult {
    let handle = match result_or_error("DeleteSymbol", editor.synthetic_handle_from_symbol_id(symbol_id)) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let op = EditOp::DeleteSymbol { handle, symbol_id: symbol_id.to_string() };
    publish_result(
        "DeleteSymbol",
        publish_edit_event(&editor.project_root, op.to_event(editor.project_root.to_string_lossy().as_ref())),
    )
}

pub fn dispatch(editor: &mut ProjectEditor, op: &ApiOp) -> ApiResult {
    match op {
        ApiOp::RenameSymbol { old, new } => handle_rename_symbol(editor, old, new),
        ApiOp::MoveSymbol { symbol_id, new_module_path } => handle_move_symbol(editor, symbol_id, new_module_path),
        ApiOp::RenameModule { old_module_path, new_name } => {
            let event = canon_types::EditEvent::RenameModule {
                project: editor.project_root.to_string_lossy().to_string(),
                old: old_module_path.to_string(),
                new: new_name.to_string(),
            };
            publish_result("RenameModule", publish_edit_event(&editor.project_root, event))
        }
        ApiOp::RenameDir { old_dir, new_dir } => {
            let event = canon_types::EditEvent::RenameDir {
                project: editor.project_root.to_string_lossy().to_string(),
                old: PathBuf::from(old_dir),
                new: PathBuf::from(new_dir),
            };
            publish_result("RenameDir", publish_edit_event(&editor.project_root, event))
        }
        ApiOp::Help | ApiOp::ListOps => {
            let ops = vec![
                "RenameSymbol",
                "MoveSymbol",
                "RenameModule",
                "RenameDir",
                "ListSymbols",
                "GetSymbol",
                "PreviewRename",
                "CheckErrors",
                "DeleteSymbol",
                "InlineModule",
                "ExtractModule",
                "SuggestRenames",
                "ApplySuggestions",
                "Help",
                "ListOps",
            ];
            let flags = serde_json::json!({
                "verify": "bool (default false)",
                "check": "bool (default false)"
            });
            let name = match op {
                ApiOp::Help => "Help",
                _ => "ListOps",
            };
            api_ok(name, Some(serde_json::json!({ "ops": ops, "flags": flags })))
        }
        ApiOp::ListSymbols => {
            let data = editor.symbol_catalog().into_iter().map(|(id, kind)| serde_json::json!({ "symbol_id": id, "kind": kind })).collect::<Vec<_>>();
            api_ok("ListSymbols", Some(serde_json::Value::Array(data)))
        }
        ApiOp::GetSymbol { symbol_id } => {
            let handle = match result_or_error("GetSymbol", editor.synthetic_handle_from_symbol_id(symbol_id)) {
                Ok(v) => v,
                Err(e) => return e,
            };
            api_ok(
                "GetSymbol",
                Some(serde_json::json!({
                    "file": handle.file,
                    "module_path": handle.module_path,
                    "name": handle.name,
                    "kind": format!("{:?}", handle.kind),
                })),
            )
        }
        ApiOp::PreviewRename => match editor.preview() {
            Ok(diff) => api_ok("PreviewRename", Some(serde_json::Value::String(diff))),
            Err(e) => api_error("PreviewRename", e),
        },
        ApiOp::CheckErrors => api_unsupported("CheckErrors", "CheckErrors is handled at the request level; use check=true"),
        ApiOp::DeleteSymbol { symbol_id } => handle_delete_symbol(editor, symbol_id),
        ApiOp::InlineModule { .. } => api_unsupported("InlineModule", "InlineModule not implemented"),
        ApiOp::ExtractModule { .. } => api_unsupported("ExtractModule", "ExtractModule not implemented"),
        ApiOp::SuggestRenames { .. } => api_unsupported("SuggestRenames", "SuggestRenames pipeline removed; not implemented"),
        ApiOp::ApplySuggestions { suggestions } => {
            let mut failures = Vec::new();
            for (old, new) in suggestions {
                let new_ident = match normalize_new_ident(new) {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(format!("{old}: {e}"));
                        continue;
                    }
                };
                let handle = match editor.synthetic_handle_from_symbol_id(old) {
                    Ok(handle) => handle,
                    Err(e) => {
                        failures.push(format!("{old}: {e}"));
                        continue;
                    }
                };
                let op = EditOp::MutateField { handle, symbol_id: old.to_string(), mutation: FieldMutation::RenameIdent(new_ident) };
                if let Err(e) = publish_edit_event(&editor.project_root, op.to_event(editor.project_root.to_string_lossy().as_ref())) {
                    failures.push(format!("{old}: {e}"));
                }
            }
            if failures.is_empty() {
                api_queued("ApplySuggestions")
            } else {
                api_result("ApplySuggestions", "partial", Some(failures.join("; ")), None)
            }
        }
    }
}

fn normalize_new_ident(new_name: &str) -> Result<String, String> {
    if !new_name.contains("::") {
        return Err("new name must be a full path like crate::module::Type".to_string());
    }
    Ok(new_name.rsplit("::").next().unwrap_or(new_name).to_string())
}
