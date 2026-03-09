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
}

#[derive(Serialize)]
pub struct ApiResult {
    pub op: String,
    pub status: String,
    pub detail: Option<String>,
}

pub fn dispatch(editor: &mut ProjectEditor, op: &ApiOp) -> ApiResult {
    match op {
        ApiOp::RenameSymbol { old, new } => match editor.queue(
            old,
            NodeOp::MutateField {
                handle: match editor.synthetic_handle_from_symbol_id(old) {
                    Ok(handle) => handle,
                    Err(e) => {
                        return ApiResult {
                            op: "RenameSymbol".into(),
                            status: "error".into(),
                            detail: Some(e.to_string()),
                        }
                    }
                },
                mutation: FieldMutation::RenameIdent(new.clone()),
            },
        ) {
            Ok(_) => ApiResult {
                op: "RenameSymbol".into(),
                status: "queued".into(),
                detail: None,
            },
            Err(e) => ApiResult {
                op: "RenameSymbol".into(),
                status: "error".into(),
                detail: Some(e.to_string()),
            },
        },
        ApiOp::MoveSymbol {
            symbol_id,
            new_module_path,
        } => match editor.queue(
            symbol_id,
            NodeOp::MoveSymbol {
                handle: match editor.synthetic_handle_from_symbol_id(symbol_id) {
                    Ok(handle) => handle,
                    Err(e) => {
                        return ApiResult {
                            op: "MoveSymbol".into(),
                            status: "error".into(),
                            detail: Some(e.to_string()),
                        }
                    }
                },
                symbol_id: symbol_id.clone(),
                new_module_path: new_module_path.clone(),
                new_crate: None,
            },
        ) {
            Ok(_) => ApiResult {
                op: "MoveSymbol".into(),
                status: "queued".into(),
                detail: None,
            },
            Err(e) => ApiResult {
                op: "MoveSymbol".into(),
                status: "error".into(),
                detail: Some(e.to_string()),
            },
        },
        ApiOp::RenameModule {
            old_module_path,
            new_name,
        } => {
            editor.queue_module_rename(old_module_path, new_name);
            ApiResult {
                op: "RenameModule".into(),
                status: "queued".into(),
                detail: None,
            }
        }
        ApiOp::RenameDir { old_dir, new_dir } => {
            editor.queue_directory_rename(&PathBuf::from(old_dir), &PathBuf::from(new_dir));
            ApiResult {
                op: "RenameDir".into(),
                status: "queued".into(),
                detail: None,
            }
        }
    }
}
