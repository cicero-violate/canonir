use super::helper::{determine_source_root, module_path_from_file, symbol_kind_from_str};
use crate::query::session::AnalysisSession;
use crate::edit::oracle::StructuralEditOracle;
use crate::edit::oracle::StructuralEditOracleApi;
use crate::symbol_index::SymbolIndex;
use crate::edit::symbol_id::normalize_symbol_id;
use crate::structured::{EditOp, FieldMutation, SymbolHandle, SymbolKind};
use anyhow::{anyhow, Result};
use proc_macro2::Span;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use syn::{Item, ItemConst, ItemEnum, ItemFn, ItemMod, ItemStatic, ItemStruct, ItemTrait, ItemType};

use super::types::{DirRename, ModuleRename, NodeRegistry, PendingEdit, ProjectEditor};

impl ProjectEditor {
    pub fn load(project: &Path) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        let session = Arc::new(SymbolIndex::build(project)?);
        Self::load_with_session_inner(project, oracle, session)
    }

    #[deprecated(note = "use ProjectEditor::load")]
    pub fn load_with_rustc(project: &Path) -> Result<Self> {
        Self::load(project)
    }

    pub fn load_with_session(project: &Path, session: Arc<SymbolIndex>) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        Self::load_with_session_inner(project, oracle, session)
    }

    fn load_with_session_inner(project: &Path, oracle: Box<dyn StructuralEditOracleApi>, session: Arc<SymbolIndex>) -> Result<Self> {
        let source_root = determine_source_root(project);
        let analysis = AnalysisSession::load(project)?;
        if analysis.files.is_empty() {
            return Err(anyhow!("analysis has no files listed for {}", project.display()));
        }
        let mut files: Vec<PathBuf> = analysis.files.iter().cloned().collect();
        files.sort();
        let mut registry = NodeRegistry::default();
        let mut original_sources = HashMap::new();
        let mut parsed_files: Vec<(PathBuf, syn::File)> = Vec::new();
        for file in files {
            let file_path = file.clone();
            let content = std::fs::read_to_string(&file_path)?;
            let ast = parse_file_lossy(&content);
            registry.asts.insert(file_path.clone(), ast);
            registry.sources.insert(file_path.clone(), content.clone());
            original_sources.insert(file_path.clone(), content);
            let stored_ast = registry
                .asts
                .get(&file_path)
                .cloned()
                .unwrap_or_else(|| syn::File { shebang: None, attrs: Vec::new(), items: Vec::new() });
            parsed_files.push((file_path.clone(), stored_ast));
        }

        let module_files: HashMap<String, PathBuf> = analysis.module_files.clone();

        // Rebuild handles + module_files using best module path per file.
        registry.handles.clear();
        registry.module_files.clear();
        registry.module_files = module_files.clone();
        for (file, ast) in &parsed_files {
            let candidates = analysis.file_modules.get(file).cloned().unwrap_or_default();
            let module_path = if candidates.is_empty() {
                module_path_from_file(&source_root, file)?
            } else {
                select_best_module_path(&candidates)
            };
            if ast.items.is_empty() {
                if let Some(content) = registry.sources.get(file) {
                    index_file_symbols_by_text(content, file, &module_path, &mut registry.handles);
                }
            } else {
                index_file_symbols(ast, file, &module_path, &mut registry.handles);
            }
        }
        if std::env::var("RENAME_DEBUG_MODULES").ok().as_deref() == Some("1") {
            let mut interesting: Vec<(String, PathBuf)> = registry.module_files.iter().filter(|(module_path, _)| module_path.contains("gpu_scheduler")).map(|(k, v)| (k.clone(), v.clone())).collect();
            interesting.sort_by(|a, b| a.0.cmp(&b.0));
            eprintln!("debug module_files gpu_scheduler:");
            for (module_path, path) in interesting {
                eprintln!("  {module_path} -> {}", path.display());
            }
        }
        Ok(Self {
            registry,
            changesets: HashMap::new(),
            oracle,
            original_sources,
            last_applied_sources: HashMap::new(),
            project_root: project.to_path_buf(),
            source_root,
            pending_module_renames: Vec::new(),
            pending_dir_renames: Vec::new(),
            pending_file_moves: Vec::new(),
            last_touched_files: HashSet::new(),
            session: Some(session),
        })
    }

    pub fn queue(&mut self, symbol_id: &str, op: EditOp) -> Result<()> {
        let norm = normalize_symbol_id(symbol_id);
        let handle = match &op {
            EditOp::MutateField { handle, .. }
            | EditOp::MoveSymbol { handle, .. }
            | EditOp::DeleteSymbol { handle, .. } => Some(handle),
        };
        if let Some(handle) = handle {
            if !self.registry.handles.contains_key(&norm) {
                self.registry.handles.insert(norm.clone(), handle.clone());
            }
        }
        let file = match &op {
            EditOp::MutateField { handle, .. }
            | EditOp::MoveSymbol { handle, .. }
            | EditOp::DeleteSymbol { handle, .. } => handle.file.clone(),
        };
        self.changesets.entry(file).or_default().push(PendingEdit { symbol_id: norm, op });
        Ok(())
    }

    pub fn queue_by_id(&mut self, symbol_id: &str, mutation: FieldMutation) -> Result<()> {
        let norm = normalize_symbol_id(symbol_id);
        let handle = if let Some(handle) = self.registry.handles.get(&norm).cloned() {
            handle
        } else if matches!(mutation, FieldMutation::RenameIdent(_)) {
            self.synthetic_handle_from_symbol_id(&norm)?
        } else {
            return Err(anyhow!("no handle found for {symbol_id}"));
        };
        let op = EditOp::MutateField { handle, mutation };
        self.queue(&norm, op)
    }

    pub fn synthetic_handle_from_symbol_id(&self, symbol_id: &str) -> Result<SymbolHandle> {
        let (module_path, name) = symbol_id.rsplit_once("::").ok_or_else(|| anyhow!("invalid symbol id: {symbol_id}"))?;
        let kind = self.session.as_ref().and_then(|session| session.symbol_kind(symbol_id)).map(symbol_kind_from_str).unwrap_or(SymbolKind::Fn);
        let file = self.registry.module_files.get(module_path).cloned().unwrap_or_else(PathBuf::new);
        Ok(SymbolHandle { file, module_path: module_path.to_string(), name: name.to_string(), kind })
    }

    pub fn has_symbol(&self, symbol_id: &str) -> bool {
        let norm = normalize_symbol_id(symbol_id);
        if let Some(session) = &self.session {
            return session.spans_for(&norm).is_some();
        }
        self.registry.handles.contains_key(&norm)
    }

    pub fn symbol_ids(&self) -> Vec<String> {
        if let Some(session) = &self.session {
            return session.symbol_ids();
        }
        self.registry.handles.keys().cloned().collect()
    }

    pub fn symbol_catalog(&self) -> Vec<(String, String)> {
        if let Some(session) = &self.session {
            return session.symbol_catalog();
        }
        self.registry
            .handles
            .values()
            .map(|handle| {
                let kind = match handle.kind {
                    SymbolKind::Fn => "fn",
                    SymbolKind::Struct => "struct",
                    SymbolKind::Enum => "enum",
                    SymbolKind::Const => "const",
                    SymbolKind::Static => "static",
                    SymbolKind::Type => "type",
                    SymbolKind::Trait => "trait",
                    SymbolKind::Module => "module",
                };
                (format!("{}::{}", handle.module_path, handle.name), kind.to_string())
            })
            .collect()
    }

    pub fn queue_module_rename(&mut self, old_module_path: &str, new_name: &str) {
        let uses_crate_prefix = self.registry.module_files.keys().any(|k| k.starts_with("crate::"));
        let norm = super::helper::normalize_module_path(old_module_path, uses_crate_prefix);
        self.pending_module_renames.push(ModuleRename { old_module_path: norm, new_name: new_name.to_string() });
    }

    pub fn queue_directory_rename(&mut self, old_dir: &Path, new_dir: &Path) {
        self.pending_dir_renames.push(DirRename { old_dir: old_dir.to_path_buf(), new_dir: new_dir.to_path_buf() });
    }
}

fn parse_file_lossy(content: &str) -> syn::File {
    syn::parse_file(content).unwrap_or_else(|_| syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: Vec::new(),
    })
}

pub(crate) fn index_file_symbols(ast: &syn::File, file: &Path, module_path: &str, handles: &mut HashMap<String, SymbolHandle>) {
    index_items(&ast.items, file, module_path, handles);
}

pub(crate) fn index_file_symbols_by_text(content: &str, file: &Path, module_path: &str, handles: &mut HashMap<String, SymbolHandle>) {
    let re = regex::Regex::new(r"(?m)^\\s*(pub\\s+)?(fn|struct|enum|const|static|type|trait|mod)\\s+([A-Za-z_][A-Za-z0-9_]*)").ok();
    let Some(re) = re else {
        return;
    };
    for cap in re.captures_iter(content) {
        let kind = match &cap[2] {
            "fn" => SymbolKind::Fn,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "const" => SymbolKind::Const,
            "static" => SymbolKind::Static,
            "type" => SymbolKind::Type,
            "trait" => SymbolKind::Trait,
            "mod" => SymbolKind::Module,
            _ => continue,
        };
        let name = &cap[3];
        let ident = syn::Ident::new(name, Span::call_site());
        insert_handle(file, module_path, &ident, kind, handles);
    }
}

pub(crate) fn index_items(items: &[Item], file: &Path, module_path: &str, handles: &mut HashMap<String, SymbolHandle>) {
    for item in items {
        match item {
            Item::Fn(ItemFn { sig, .. }) => {
                insert_handle(file, module_path, &sig.ident, SymbolKind::Fn, handles);
            }
            Item::Struct(ItemStruct { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Struct, handles);
            }
            Item::Enum(ItemEnum { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Enum, handles);
            }
            Item::Const(ItemConst { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Const, handles);
            }
            Item::Static(ItemStatic { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Static, handles);
            }
            Item::Type(ItemType { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Type, handles);
            }
            Item::Trait(ItemTrait { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Trait, handles);
            }
            Item::Mod(ItemMod { ident, content, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Module, handles);
                if let Some((_, inner_items)) = content {
                    let next_module_path = format!("{module_path}::{}", ident);
                    index_items(inner_items, file, &next_module_path, handles);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn insert_handle(file: &Path, module_path: &str, ident: &syn::Ident, kind: SymbolKind, handles: &mut HashMap<String, SymbolHandle>) {
    let symbol_id = format!("{module_path}::{ident}");
    handles.insert(symbol_id, SymbolHandle { file: file.to_path_buf(), module_path: module_path.to_string(), name: ident.to_string(), kind });
}

#[allow(dead_code)]
fn index_module_files(ast: &syn::File, file: &Path, module_path: &str, module_files: &mut HashMap<String, PathBuf>) {
    let base_dir = file.parent().unwrap_or_else(|| Path::new(""));
    for item in &ast.items {
        let Item::Mod(item_mod) = item else { continue };
        let mod_name = item_mod.ident.to_string();
        let mod_path = if module_path == "crate" { format!("crate::{mod_name}") } else { format!("{module_path}::{mod_name}") };
        if let Some(path_lit) = module_path_attr_value(item_mod) {
            let path = if Path::new(&path_lit).is_absolute() { PathBuf::from(&path_lit) } else { base_dir.join(&path_lit) };
            module_files.insert(mod_path, path);
            continue;
        }

        // No #[path] attribute: try conventional module files.
        let direct = base_dir.join(format!("{mod_name}.rs"));
        if direct.is_file() {
            module_files.insert(mod_path, direct);
            continue;
        }
        let nested = base_dir.join(&mod_name).join("mod.rs");
        if nested.is_file() {
            module_files.insert(mod_path, nested);
            continue;
        }

        // "{mod_name}_mod.rs" acts as module root (equivalent to mod.rs in a subdir)
        let prefix_mod_file = base_dir.join(format!("{mod_name}_mod.rs"));
        if prefix_mod_file.is_file() {
            module_files.insert(mod_path, prefix_mod_file);
            continue;
        }

        // Legacy fallback: match files like capability_<mod>.rs
        let mut candidates = Vec::new();
        if let Ok(entries) = std::fs::read_dir(base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == format!("{mod_name}.rs") || name.ends_with(&format!("_{mod_name}.rs")) {
                    candidates.push(path);
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        if candidates.len() == 1 {
            module_files.insert(mod_path, candidates.remove(0));
        }
    }
}

#[allow(dead_code)]
fn module_path_attr_value(item_mod: &ItemMod) -> Option<String> {
    for attr in &item_mod.attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        let syn::Meta::NameValue(name_value) = &attr.meta else { continue };
        let syn::Expr::Lit(expr_lit) = &name_value.value else { continue };
        let syn::Lit::Str(lit) = &expr_lit.lit else { continue };
        return Some(lit.value());
    }
    None
}

#[allow(dead_code)]
fn module_path_for_file(module_files: &HashMap<String, PathBuf>, source_root: &Path, file: &Path) -> Result<String> {
    let candidates = module_paths_for_file(module_files, file);
    if candidates.is_empty() {
        return module_path_from_file(source_root, file);
    }
    Ok(select_best_module_path(&candidates))
}

#[allow(dead_code)]
fn module_paths_for_file(module_files: &HashMap<String, PathBuf>, file: &Path) -> Vec<String> {
    let file_canon = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    module_files
        .iter()
        .filter_map(|(module_path, path)| {
            if path == file {
                return Some(module_path.clone());
            }
            let path_canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if path_canon == file_canon {
                Some(module_path.clone())
            } else {
                None
            }
        })
        .collect()
}

fn select_best_module_path(candidates: &[String]) -> String {
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|a, b| {
        let score_a = module_path_score(a);
        let score_b = module_path_score(b);
        score_b
            .cmp(&score_a)
            .then_with(|| b.len().cmp(&a.len()))
            .then_with(|| a.cmp(b))
    });
    sorted[0].clone()
}

fn module_path_score(path: &str) -> i32 {
    let last = path.rsplit("::").next().unwrap_or("");
    if last == "mod" || last.ends_with("_mod") {
        0
    } else {
        1
    }
}
