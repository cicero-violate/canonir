use crate::core::oracle::StructuralEditOracleApi;
use crate::core::rustc_session::RustcSession;
use crate::structured::{NodeOp, SymbolHandle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EditConflict {
    pub symbol_id: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ChangeReport {
    pub touched_files: Vec<PathBuf>,
    pub conflicts: Vec<EditConflict>,
    pub file_moves: Vec<(PathBuf, PathBuf)>,
}

#[derive(Default)]
pub(crate) struct NodeRegistry {
    pub asts: HashMap<PathBuf, syn::File>,
    pub sources: HashMap<PathBuf, String>,
    pub handles: HashMap<String, SymbolHandle>,
    pub module_files: HashMap<String, PathBuf>,
}

pub struct ProjectEditor {
    pub registry: NodeRegistry,
    pub changesets: HashMap<PathBuf, Vec<QueuedOp>>,
    pub oracle: Box<dyn StructuralEditOracleApi>,
    pub original_sources: HashMap<PathBuf, String>,
    pub project_root: PathBuf,
    pub source_root: PathBuf,
    pub pending_module_renames: Vec<ModuleRename>,
    pub pending_dir_renames: Vec<DirRename>,
    pub pending_file_moves: Vec<(PathBuf, PathBuf)>,
    pub last_touched_files: HashSet<PathBuf>,
    pub session: Option<Arc<RustcSession>>,
}

#[derive(Clone)]
pub(crate) struct QueuedOp {
    pub symbol_id: String,
    pub op: NodeOp,
}

#[derive(Clone)]
pub(crate) struct ModuleRename {
    pub old_module_path: String,
    pub new_name: String,
}

#[derive(Clone)]
pub(crate) struct DirRename {
    pub old_dir: PathBuf,
    pub new_dir: PathBuf,
}
