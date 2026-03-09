//
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::ProjectEditor;
use crate::structured::{FieldMutation, NodeOp};

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

pub fn dispatch(editor: &mut ProjectEditor, op: &ApiOp) -> ApiResult {
    match op {
        ApiOp::RenameSymbol { old, new } => {
            let new_ident = match normalize_new_ident(new) {
                Ok(v) => v,
                Err(e) => return ApiResult { op: "RenameSymbol".into(), status: "error".into(), detail: Some(e), data: None },
            };
            match editor.queue(
                old,
                NodeOp::MutateField {
                    handle: match editor.synthetic_handle_from_symbol_id(old) {
                        Ok(handle) => handle,
                        Err(e) => return ApiResult { op: "RenameSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
                    },
                    mutation: FieldMutation::RenameIdent(new_ident),
                },
            ) {
                Ok(_) => ApiResult { op: "RenameSymbol".into(), status: "queued".into(), detail: None, data: None },
                Err(e) => ApiResult { op: "RenameSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
            }
        }
        ApiOp::MoveSymbol { symbol_id, new_module_path } => match editor.queue(
            symbol_id,
            NodeOp::MoveSymbol {
                handle: match editor.synthetic_handle_from_symbol_id(symbol_id) {
                    Ok(handle) => handle,
                    Err(e) => return ApiResult { op: "MoveSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
                },
                symbol_id: symbol_id.clone(),
                new_module_path: new_module_path.clone(),
                new_crate: None,
            },
        ) {
            Ok(_) => ApiResult { op: "MoveSymbol".into(), status: "queued".into(), detail: None, data: None },
            Err(e) => ApiResult { op: "MoveSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
        },
        ApiOp::RenameModule { old_module_path, new_name } => {
            editor.queue_module_rename(old_module_path, new_name);
            ApiResult { op: "RenameModule".into(), status: "queued".into(), detail: None, data: None }
        }
        ApiOp::RenameDir { old_dir, new_dir } => {
            editor.queue_directory_rename(&PathBuf::from(old_dir), &PathBuf::from(new_dir));
            ApiResult { op: "RenameDir".into(), status: "queued".into(), detail: None, data: None }
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
            ApiResult {
                op: match op {
                    ApiOp::Help => "Help",
                    _ => "ListOps",
                }
                .into(),
                status: "ok".into(),
                detail: None,
                data: Some(serde_json::json!({ "ops": ops, "flags": flags })),
            }
        }
        ApiOp::ListSymbols => {
            let data = editor.symbol_catalog().into_iter().map(|(id, kind)| serde_json::json!({ "symbol_id": id, "kind": kind })).collect::<Vec<_>>();
            ApiResult { op: "ListSymbols".into(), status: "ok".into(), detail: None, data: Some(serde_json::Value::Array(data)) }
        }
        ApiOp::GetSymbol { symbol_id } => match editor.synthetic_handle_from_symbol_id(symbol_id) {
            Ok(handle) => ApiResult {
                op: "GetSymbol".into(),
                status: "ok".into(),
                detail: None,
                data: Some(serde_json::json!({
                    "file": handle.file,
                    "module_path": handle.module_path,
                    "name": handle.name,
                    "kind": format!("{:?}", handle.kind),
                })),
            },
            Err(e) => ApiResult { op: "GetSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
        },
        ApiOp::PreviewRename => match editor.preview() {
            Ok(diff) => ApiResult { op: "PreviewRename".into(), status: "ok".into(), detail: None, data: Some(serde_json::Value::String(diff)) },
            Err(e) => ApiResult { op: "PreviewRename".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
        },
        ApiOp::CheckErrors => ApiResult { op: "CheckErrors".into(), status: "unsupported".into(), detail: Some("CheckErrors is handled at the request level; use check=true".into()), data: None },
        ApiOp::DeleteSymbol { symbol_id } => {
            let handle = match editor.synthetic_handle_from_symbol_id(symbol_id) {
                Ok(handle) => handle,
                Err(e) => return ApiResult { op: "DeleteSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
            };
            match editor.queue(symbol_id, NodeOp::DeleteSymbol { handle, symbol_id: symbol_id.clone() }) {
                Ok(_) => ApiResult { op: "DeleteSymbol".into(), status: "queued".into(), detail: None, data: None },
                Err(e) => ApiResult { op: "DeleteSymbol".into(), status: "error".into(), detail: Some(e.to_string()), data: None },
            }
        }
        ApiOp::InlineModule { .. } => ApiResult { op: "InlineModule".into(), status: "unsupported".into(), detail: Some("InlineModule not implemented".into()), data: None },
        ApiOp::ExtractModule { .. } => ApiResult { op: "ExtractModule".into(), status: "unsupported".into(), detail: Some("ExtractModule not implemented".into()), data: None },
        ApiOp::SuggestRenames { .. } => ApiResult { op: "SuggestRenames".into(), status: "unsupported".into(), detail: Some("SuggestRenames pipeline removed; not implemented".into()), data: None },
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
                let op = NodeOp::MutateField { handle, mutation: FieldMutation::RenameIdent(new_ident) };
                if let Err(e) = editor.queue(old, op) {
                    failures.push(format!("{old}: {e}"));
                }
            }
            if failures.is_empty() {
                ApiResult { op: "ApplySuggestions".into(), status: "queued".into(), detail: None, data: None }
            } else {
                ApiResult { op: "ApplySuggestions".into(), status: "partial".into(), detail: Some(failures.join("; ")), data: None }
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
  A      P¶ ^ÅW  ÀS^aÅW                  a.k.a. "adjust") the font size of all faces by INCREMENT.

Interactively, INCREMENT is the prefix numeric argument, and defaults
to 1.  Positive values of INCREMENT increase the font size, negative
values decrease it.

When you invoke this command, it performs the initial change of the
font size, and after that allows further changes by typing one of the
following keys immediately after invoking the command:

   +, =   Globally increase the height of the default face
   -      Globally decrease the height of the default face
   0      Globally reset the height of the default face

(The change of the font size produced by these keys depends on the
final component of the key sequence, with all modifiers removed.)

Buffer-local face adjustments have higher priority than global
face adjustments.

The variable â€˜global-text-scale-adjust-resizes-framesâ€™ controls
whether the frames are resized to keep the same number of lines
and characters per line when the font size is adjusted.

See also the related command â€˜text-scale-adjustâ€™.  Unlike that
command, which scales the font size with a factor,
â€˜global-text-scale-adjustâ€™ scales the font size with an
increment.

(fn INCREMENT)   à      @       €œÆaÅW  ˆ¹‘!ÅW   c^aÅW  ÀwaÅW  ¸‡aÅW         ÿÿÿÿÿÿÿÿ!       °œÆaÅW  à@Ğ`ÅW  @             ĞÅYaÅW                          ğG^ÅW                         5       ÿÿÿÿÿÿÿÿ        °»ïaÅW   ğG^ÅW                         6       ÿÿÿÿÿÿÿÿ        p}aÅW  `ğG^ÅW                          ğG^ÅW                                ÿÿÿÿÿÿÿÿ        »ïaÅW  ÀğG^ÅW                          ñG^ÅW                          ñG^ÅW                                ÿÿÿÿÿÿÿÿ        p»ïaÅW  @ñG^ÅW                         €ñG^ÅW                          ñG^ÅW                         5       ÿÿÿÿÿÿÿÿ        0»ïaÅW  ÀñG^ÅW                          òG^ÅW                          òG^ÅW                                ÿÿÿÿÿÿÿÿ        »ïaÅW  @òG^ÅW                         €òG^ÅW                         @       ÿÿÿÿÿÿÿÿ        °}aÅW   òG^ÅW  	                              ÿÿÿÿÿÿÿÿ        àºïaÅW  àòG^ÅW                          óG^ÅW                         @óG^ÅW                                ÿÿÿÿÿÿÿÿ        ¸ºïaÅW  `óG^ÅW                          óG^ÅW                         ÂYaÅW  ØèS'z                     ÿÿÿÿÿÿÿÿÿÿÿÿ   ÿÿÿÿÿÿÿÿÿÿÿÿÿÿÿ