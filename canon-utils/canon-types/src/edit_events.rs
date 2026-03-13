use serde::{Deserialize, Serialize};
use std::path::PathBuf;
//
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditEvent {
    RenameSymbol { project: String, old: String, new: String },
    MoveSymbol { project: String, symbol: String, module: String },
    DeleteSymbol { project: String, symbol: String },
    RenameModule { project: String, old: String, new: String },
    RenameDir { project: String, old: PathBuf, new: PathBuf },
    InlineModule { project: String, module: String },
    ExtractModule { project: String, symbol: String, module: String },
}
