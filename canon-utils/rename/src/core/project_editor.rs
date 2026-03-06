use crate::core::oracle::StructuralEditOracle;
use crate::core::oracle::StructuralEditOracleApi;
use crate::core::rustc_session::{RustcSession, SpanRange};
use crate::core::symbol_id::normalize_symbol_id;
use crate::core::syn_patcher;
use crate::fs::collect_rs_files;
use crate::structured::{FieldMutation, NodeOp, SymbolHandle, SymbolKind};
use anyhow::{anyhow, Context, Result};
use proc_macro2::Span;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use syn::visit_mut::VisitMut;
use syn::{
    Item, ItemConst, ItemEnum, ItemFn, ItemMod, ItemStatic, ItemStruct, ItemTrait,
    ItemType,
};
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
struct NodeRegistry {
    asts: HashMap<PathBuf, syn::File>,
    sources: HashMap<PathBuf, String>,
    handles: HashMap<String, SymbolHandle>,
    module_files: HashMap<String, PathBuf>,
}
pub struct ProjectEditor {
    pub registry: NodeRegistry,
    pub changesets: HashMap<PathBuf, Vec<QueuedOp>>,
    pub oracle: Box<dyn StructuralEditOracleApi>,
    pub original_sources: HashMap<PathBuf, String>,
    project_root: PathBuf,
    source_root: PathBuf,
    pending_module_renames: Vec<ModuleRename>,
    pending_dir_renames: Vec<DirRename>,
    pending_file_moves: Vec<(PathBuf, PathBuf)>,
    last_touched_files: HashSet<PathBuf>,
    session: Option<Arc<RustcSession>>,
}
#[derive(Clone)]
pub(crate) struct QueuedOp {
    pub symbol_id: String,
    pub op: NodeOp,
}
#[derive(Clone)]
struct ModuleRename {
    old_module_path: String,
    new_name: String,
}
#[derive(Clone)]
struct DirRename {
    old_dir: PathBuf,
    new_dir: PathBuf,
}
impl ProjectEditor {
    pub fn load_with_rustc(project: &Path) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        let session = Arc::new(RustcSession::build(project)?);
        Self::load_with_session_inner(project, oracle, session)
    }
    pub fn load_with_session(project: &Path, session: Arc<RustcSession>) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        Self::load_with_session_inner(project, oracle, session)
    }
    fn load_with_session_inner(
        project: &Path,
        oracle: Box<dyn StructuralEditOracleApi>,
        session: Arc<RustcSession>,
    ) -> Result<Self> {
        let source_root = determine_source_root(project);
        let files = collect_rs_files(&source_root)?;
        let mut registry = NodeRegistry::default();
        let mut original_sources = HashMap::new();
        for file in files {
            let content = std::fs::read_to_string(&file)?;
            let module_path = module_path_from_file(&source_root, &file)?;
            registry.module_files.insert(module_path.clone(), file.clone());
            let ast = match syn::parse_file(&content) {
                Ok(ast) => {
                    index_file_symbols(&ast, &file, &module_path, &mut registry.handles);
                    ast
                }
                Err(_) => {
                    index_file_symbols_by_text(
                        &content,
                        &file,
                        &module_path,
                        &mut registry.handles,
                    );
                    syn::File {
                        shebang: None,
                        attrs: Vec::new(),
                        items: Vec::new(),
                    }
                }
            };
            registry.asts.insert(file.clone(), ast);
            registry.sources.insert(file.clone(), content.clone());
            original_sources.insert(file, content);
        }
        Ok(Self {
            registry,
            changesets: HashMap::new(),
            oracle,
            original_sources,
            project_root: project.to_path_buf(),
            source_root,
            pending_module_renames: Vec::new(),
            pending_dir_renames: Vec::new(),
            pending_file_moves: Vec::new(),
            last_touched_files: HashSet::new(),
            session: Some(session),
        })
    }
    pub fn queue(&mut self, symbol_id: &str, op: NodeOp) -> Result<()> {
        let norm = crate::core::symbol_id::normalize_symbol_id(symbol_id);
        let handle = match &op {
            NodeOp::MutateField { handle, .. } | NodeOp::MoveSymbol { handle, .. } => {
                Some(handle)
            }
        };
        if let Some(handle) = handle {
            if !self.registry.handles.contains_key(&norm) {
                self.registry.handles.insert(norm.clone(), handle.clone());
            }
        }
        let file = match &op {
            NodeOp::MutateField { handle, .. } | NodeOp::MoveSymbol { handle, .. } => {
                handle.file.clone()
            }
        };
        self.changesets.entry(file).or_default().push(QueuedOp { symbol_id: norm, op });
        Ok(())
    }
    pub fn queue_by_id(
        &mut self,
        symbol_id: &str,
        mutation: FieldMutation,
    ) -> Result<()> {
        let norm = crate::core::symbol_id::normalize_symbol_id(symbol_id);
        let handle = if let Some(handle) = self.registry.handles.get(&norm).cloned() {
            handle
        } else if matches!(mutation, FieldMutation::RenameIdent(_)) {
            self.synthetic_handle_from_symbol_id(&norm)?
        } else {
            return Err(anyhow!("no handle found for {symbol_id}"));
        };
        let op = NodeOp::MutateField {
            handle,
            mutation,
        };
        self.queue(&norm, op)
    }
    fn synthetic_handle_from_symbol_id(&self, symbol_id: &str) -> Result<SymbolHandle> {
        let (module_path, name) = symbol_id
            .rsplit_once("::")
            .ok_or_else(|| anyhow!("invalid symbol id: {symbol_id}"))?;
        let kind = self
            .session
            .as_ref()
            .and_then(|session| session.symbol_kind(symbol_id))
            .map(symbol_kind_from_str)
            .unwrap_or(SymbolKind::Fn);
        let file = self
            .registry
            .module_files
            .get(module_path)
            .cloned()
            .unwrap_or_else(PathBuf::new);
        Ok(SymbolHandle {
            file,
            module_path: module_path.to_string(),
            name: name.to_string(),
            kind,
        })
    }
    pub fn has_symbol(&self, symbol_id: &str) -> bool {
        let norm = crate::core::symbol_id::normalize_symbol_id(symbol_id);
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
            .iter()
            .map(|(id, handle)| {
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
                (id.clone(), kind.to_string())
            })
            .collect()
    }
    pub fn queue_module_rename(&mut self, old_module_path: &str, new_name: &str) {
        self.pending_module_renames
            .push(ModuleRename {
                old_module_path: old_module_path.to_string(),
                new_name: new_name.to_string(),
            });
    }
    pub fn queue_directory_rename(&mut self, old_dir: &Path, new_dir: &Path) {
        self.pending_dir_renames
            .push(DirRename {
                old_dir: old_dir.to_path_buf(),
                new_dir: new_dir.to_path_buf(),
            });
    }
    pub fn apply(&mut self) -> Result<ChangeReport> {
        let mut touched_files: HashSet<PathBuf> = HashSet::new();
        let mut conflicts = Vec::new();
        let mut file_moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        let module_renames: Vec<ModuleRename> = self
            .pending_module_renames
            .drain(..)
            .collect();
        for module_rename in module_renames {
            let (touched, moves) = self
                .apply_module_rename(
                    &module_rename.old_module_path,
                    &module_rename.new_name,
                )?;
            touched_files.extend(touched);
            file_moves.extend(moves);
        }
        let dir_renames: Vec<DirRename> = self.pending_dir_renames.drain(..).collect();
        for dir_rename in dir_renames {
            let (touched, moves) = self
                .apply_dir_rename(&dir_rename.old_dir, &dir_rename.new_dir)?;
            touched_files.extend(touched);
            file_moves.extend(moves);
        }
        let mut queued_ops = Vec::new();
        let changesets = std::mem::take(&mut self.changesets);
        for (_file, ops) in changesets {
            queued_ops.extend(ops);
        }
        for queued in queued_ops {
            if !self.oracle.allow_symbol(&queued.symbol_id) {
                conflicts
                    .push(EditConflict {
                        symbol_id: queued.symbol_id.clone(),
                        reason: "oracle rejected edit".to_string(),
                    });
                continue;
            }
            match &queued.op {
                NodeOp::MutateField { handle, mutation } => {
                    match mutation {
                        FieldMutation::RenameIdent(new_name) => {
                            let touched = self.apply_symbol_rename(&queued.symbol_id, new_name)?;
                            touched_files.extend(touched);
                        }
                    }
                }
                NodeOp::MoveSymbol { handle, new_module_path, .. } => {
                    let touched = self.apply_move_symbol(handle, new_module_path)?;
                    touched_files.extend(touched);
                }
            }
        }
        self.pending_file_moves.extend(file_moves.clone());
        self.last_touched_files = touched_files.clone();
        self.rewrite_sources_for(&touched_files)?;
        self.rebuild_registry()?;
        Ok(ChangeReport {
            touched_files: touched_files.into_iter().collect(),
            conflicts,
            file_moves,
        })
    }
    pub fn validate(&self) -> Result<Vec<EditConflict>> {
        let mut conflicts = Vec::new();
        for (_file, ops) in &self.changesets {
            for queued in ops {
                match &queued.op {
                    NodeOp::MutateField { handle, mutation } => {
                        if let FieldMutation::RenameIdent(new_name) = mutation {
                            let module = &handle.module_path;
                            let candidate = format!("{module}::{new_name}");
                            if self.registry.handles.contains_key(&candidate) {
                                conflicts
                                    .push(EditConflict {
                                        symbol_id: queued.symbol_id.clone(),
                                        reason: format!(
                                            "rename would conflict with existing symbol {candidate}"
                                        ),
                                    });
                            }
                        }
                    }
                    NodeOp::MoveSymbol { handle, new_module_path, .. } => {
                        let candidate = format!("{new_module_path}::{}", handle.name);
                        if self.registry.handles.contains_key(&candidate) {
                            conflicts
                                .push(EditConflict {
                                    symbol_id: queued.symbol_id.clone(),
                                    reason: format!(
                                        "move would conflict with existing symbol {candidate}"
                                    ),
                                });
                        }
                    }
                }
            }
        }
        Ok(conflicts)
    }
    pub fn preview(&self) -> Result<String> {
        let mut out = String::new();
        if !self.pending_file_moves.is_empty() {
            out.push_str("file moves:\n");
            for (from, to) in &self.pending_file_moves {
                out.push_str(&format!("- {} -> {}\n", from.display(), to.display()));
            }
            out.push('\n');
        }
        for path in &self.last_touched_files {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = if ast.items.is_empty() {
                    self.registry.sources.get(path).cloned().unwrap_or_default()
                } else {
                    prettyplease::unparse(ast)
                };
                out.push_str(&format!("=== {} ===\n", path.display()));
                out.push_str(&rendered);
                out.push('\n');
            }
        }
        Ok(out)
    }
    pub fn commit(&self) -> Result<Vec<PathBuf>> {
        for (from, to) in &self.pending_file_moves {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(from, to)?;
        }
        let mut written = Vec::new();
        for path in &self.last_touched_files {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = if ast.items.is_empty() {
                    self.registry.sources.get(path).cloned().unwrap_or_default()
                } else {
                    prettyplease::unparse(ast)
                };
                std::fs::write(path, rendered)?;
                written.push(path.clone());
            } else if let Some(source) = self.registry.sources.get(path) {
                std::fs::write(path, source)?;
                written.push(path.clone());
            }
        }
        Ok(written)
    }
    fn apply_symbol_rename(
        &mut self,
        symbol_id: &str,
        new_name: &str,
    ) -> Result<HashSet<PathBuf>> {
        if let Some(session) = &self.session {
            let norm = normalize_symbol_id(symbol_id);
            let occurrences = session
                .spans_for(&norm)
                .ok_or_else(|| anyhow!("symbol not found via rustc: {symbol_id}"))?
                .clone();
            let touched =
                self.apply_symbol_rename_with_occurrences(&occurrences, new_name)?;
            return Ok(touched);
        }
        Err(anyhow!(
            "rustc session not initialized; use ProjectEditor::load_with_rustc"
        ))
    }
    fn apply_symbol_rename_with_occurrences(
        &mut self,
        occurrences: &std::collections::HashMap<PathBuf, Vec<SpanRange>>,
        new_name: &str,
    ) -> Result<HashSet<PathBuf>> {
        let mut touched = HashSet::new();
        for (path, spans) in occurrences {
            let source = match self.registry.sources.get(path) {
                Some(content) => content.clone(),
                None => continue,
            };
            let updated = syn_patcher::patch_file(&source, spans, new_name)?;
            if updated != source {
                self.registry.sources.insert(path.to_path_buf(), updated.clone());
                if let Ok(ast) = syn::parse_file(&updated) {
                    self.registry.asts.insert(path.to_path_buf(), ast);
                }
                touched.insert(path.to_path_buf());
            }
        }
        Ok(touched)
    }
    fn apply_move_symbol(
        &mut self,
        handle: &SymbolHandle,
        new_module_path: &str,
    ) -> Result<HashSet<PathBuf>> {
        let mut touched = HashSet::new();
        let old_file = handle.file.clone();
        let old_module_path = handle.module_path.clone();
        let symbol_name = handle.name.clone();
        let removed = {
            let old_ast = self
                .registry
                .asts
                .get_mut(&old_file)
                .ok_or_else(|| anyhow!("missing AST for {}", old_file.display()))?;
            let removed = remove_top_level_item(old_ast, &symbol_name, &handle.kind)
                .ok_or_else(|| anyhow!("symbol not found for move: {}", symbol_name))?;
            touched.insert(old_file.clone());
            removed
        };
        let new_file = self
            .registry
            .module_files
            .get(new_module_path)
            .cloned()
            .ok_or_else(|| anyhow!("no file for module path {new_module_path}"))?;
        {
            let new_ast = self
                .registry
                .asts
                .get_mut(&new_file)
                .ok_or_else(|| anyhow!("missing AST for {}", new_file.display()))?;
            new_ast.items.push(removed);
            touched.insert(new_file.clone());
        }
        let old_full = build_full_path(&old_module_path, &symbol_name);
        let new_full = build_full_path(new_module_path, &symbol_name);
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_full(&old_full, &new_full);
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        Ok(touched)
    }
    fn apply_module_rename(
        &mut self,
        old_module_path: &str,
        new_name: &str,
    ) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let mut touched = HashSet::new();
        let mut file_moves = Vec::new();
        let old_segments = split_module_path(old_module_path);
        if old_segments.len() < 2 {
            return Err(anyhow!("module path must include crate and name"));
        }
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        let old_name = old_segments.last().unwrap().to_string();
        let mut new_segments = parent_segments.to_vec();
        new_segments.push(new_name.to_string());
        let new_module_path = join_module_path(&new_segments);
        if let Some(parent_file) = self
            .registry
            .module_files
            .get(&parent_module_path)
            .cloned()
        {
            if let Some(ast) = self.registry.asts.get_mut(&parent_file) {
                if rename_mod_decl(ast, &old_name, new_name) {
                    touched.insert(parent_file);
                }
            }
        }
        if let Some(module_file) = self
            .registry
            .module_files
            .get(old_module_path)
            .cloned()
        {
            if module_file.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
                if let Some(parent) = module_file.parent() {
                    if let Some(grand) = parent.parent() {
                        let new_dir = grand.join(new_name);
                        file_moves.push((parent.to_path_buf(), new_dir));
                    }
                }
            } else {
                let new_file = module_file.with_file_name(format!("{new_name}.rs"));
                file_moves.push((module_file, new_file));
            }
        }
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_prefix(
                &old_segments,
                &new_segments,
            );
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        Ok((touched, file_moves))
    }
    fn apply_dir_rename(
        &mut self,
        old_dir: &Path,
        new_dir: &Path,
    ) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let mut touched = HashSet::new();
        let mut file_moves = Vec::new();
        let old_dir = canonicalize_relative(old_dir, &self.source_root)?;
        let new_dir = canonicalize_relative(new_dir, &self.source_root)?;
        let old_segments = module_path_for_dir(&self.source_root, &old_dir)?;
        let new_segments = module_path_for_dir(&self.source_root, &new_dir)?;
        if old_segments.len() < 2 || new_segments.len() < 2 {
            return Err(anyhow!("directory rename must be under source root"));
        }
        let old_name = old_segments.last().unwrap().to_string();
        let new_name = new_segments.last().unwrap().to_string();
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        if let Some(parent_file) = self
            .registry
            .module_files
            .get(&parent_module_path)
            .cloned()
        {
            if let Some(ast) = self.registry.asts.get_mut(&parent_file) {
                if rename_mod_decl(ast, &old_name, &new_name) {
                    touched.insert(parent_file);
                }
            }
        }
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_prefix(
                &old_segments,
                &new_segments,
            );
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        file_moves.push((old_dir, new_dir));
        Ok((touched, file_moves))
    }
    fn rewrite_sources_for(&mut self, touched: &HashSet<PathBuf>) -> Result<()> {
        for path in touched {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = prettyplease::unparse(ast);
                self.registry.sources.insert(path.clone(), rendered);
            }
        }
        Ok(())
    }
    fn rebuild_registry(&mut self) -> Result<()> {
        self.registry.handles.clear();
        self.registry.module_files.clear();
        for (file, ast) in &self.registry.asts {
            let module_path = module_path_from_file(&self.source_root, file)?;
            self.registry.module_files.insert(module_path.clone(), file.clone());
            index_file_symbols(ast, file, &module_path, &mut self.registry.handles);
        }
        Ok(())
    }
}
fn determine_source_root(project: &Path) -> PathBuf {
    let src = project.join("src");
    if src.is_dir() { src } else { project.to_path_buf() }
}
fn module_path_from_file(root: &Path, file: &Path) -> Result<String> {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let mut components: Vec<String> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    if components.is_empty() {
        return Err(anyhow!("cannot derive module path for {}", file.display()));
    }
    let filename = components.pop().unwrap();
    let module_segments = if filename == "lib.rs" || filename == "main.rs" {
        components
    } else if filename == "mod.rs" {
        components
    } else {
        let stem = filename.trim_end_matches(".rs").to_string();
        let mut segs = components;
        segs.push(stem);
        segs
    };
    let mut path = String::from("crate");
    for segment in module_segments {
        if !segment.is_empty() {
            path.push_str("::");
            path.push_str(&segment);
        }
    }
    Ok(path)
}
fn module_path_for_dir(root: &Path, dir: &Path) -> Result<Vec<String>> {
    let rel = dir
        .strip_prefix(root)
        .with_context(|| {
            format!("directory {} is not under {}", dir.display(), root.display())
        })?;
    let mut segments: Vec<String> = vec!["crate".to_string()];
    for component in rel.components() {
        if let Some(s) = component.as_os_str().to_str() {
            if !s.is_empty() {
                segments.push(s.to_string());
            }
        }
    }
    Ok(segments)
}
fn canonicalize_relative(path: &Path, root: &Path) -> Result<PathBuf> {
    if path.is_absolute() { Ok(path.to_path_buf()) } else { Ok(root.join(path)) }
}
fn index_file_symbols(
    ast: &syn::File,
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    index_items(&ast.items, file, module_path, handles);
}
fn index_file_symbols_by_text(
    content: &str,
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    let re = regex::Regex::new(
            r"(?m)^\s*(pub\s+)?(fn|struct|enum|const|static|type|trait|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
        )
        .ok();
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
fn index_items(
    items: &[Item],
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
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
fn insert_handle(
    file: &Path,
    module_path: &str,
    ident: &syn::Ident,
    kind: SymbolKind,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    let symbol_id = format!("{module_path}::{ident}");
    handles
        .insert(
            symbol_id,
            SymbolHandle {
                file: file.to_path_buf(),
                module_path: module_path.to_string(),
                name: ident.to_string(),
                kind,
            },
        );
}
fn rename_item_in_file(
    ast: &mut syn::File,
    handle: &SymbolHandle,
    new_name: &str,
) -> bool {
    let mut changed = false;
    for item in &mut ast.items {
        match (item, &handle.kind) {
            (Item::Fn(item_fn), SymbolKind::Fn) if item_fn.sig.ident == handle.name => {
                item_fn.sig.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Struct(item_struct),
                SymbolKind::Struct,
            ) if item_struct.ident == handle.name => {
                item_struct.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Enum(item_enum),
                SymbolKind::Enum,
            ) if item_enum.ident == handle.name => {
                item_enum.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Const(item_const),
                SymbolKind::Const,
            ) if item_const.ident == handle.name => {
                item_const.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Static(item_static),
                SymbolKind::Static,
            ) if item_static.ident == handle.name => {
                item_static.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Type(item_type),
                SymbolKind::Type,
            ) if item_type.ident == handle.name => {
                item_type.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Trait(item_trait),
                SymbolKind::Trait,
            ) if item_trait.ident == handle.name => {
                item_trait.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            (
                Item::Mod(item_mod),
                SymbolKind::Module,
            ) if item_mod.ident == handle.name => {
                item_mod.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
            _ => {}
        }
    }
    changed
}
fn rewrite_string_attrs_in_file(
    ast: &mut syn::File,
    old_name: &str,
    new_name: &str,
) -> bool {
    struct AttrStringRewriter<'a> {
        old: &'a str,
        new: &'a str,
        changed: bool,
    }

    impl VisitMut for AttrStringRewriter<'_> {
        fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
            if let syn::Meta::List(ref mut meta_list) = attr.meta {
                let tokens = meta_list.tokens.clone();
                let mut new_tokens = proc_macro2::TokenStream::new();
                let mut token_iter = tokens.into_iter().peekable();
                let mut local_changed = false;
                while let Some(tt) = token_iter.next() {
                    match &tt {
                        proc_macro2::TokenTree::Literal(lit) => {
                            let s = lit.to_string();
                            if s == format!("\"{}\"", self.old) {
                                let new_lit = proc_macro2::Literal::string(self.new);
                                new_tokens.extend(std::iter::once(
                                    proc_macro2::TokenTree::Literal(new_lit),
                                ));
                                local_changed = true;
                                continue;
                            }
                        }
                        _ => {}
                    }
                    new_tokens.extend(std::iter::once(tt));
                }
                if local_changed {
                    meta_list.tokens = new_tokens;
                    self.changed = true;
                }
            }
            syn::visit_mut::visit_attribute_mut(self, attr);
        }
    }

    let mut rewriter = AttrStringRewriter {
        old: old_name,
        new: new_name,
        changed: false,
    };
    rewriter.visit_file_mut(ast);
    rewriter.changed
}
fn remove_top_level_item(
    ast: &mut syn::File,
    name: &str,
    kind: &SymbolKind,
) -> Option<Item> {
    let mut idx = None;
    for (i, item) in ast.items.iter().enumerate() {
        let matches = match (item, kind) {
            (Item::Fn(item_fn), SymbolKind::Fn) => item_fn.sig.ident == name,
            (Item::Struct(item_struct), SymbolKind::Struct) => item_struct.ident == name,
            (Item::Enum(item_enum), SymbolKind::Enum) => item_enum.ident == name,
            (Item::Const(item_const), SymbolKind::Const) => item_const.ident == name,
            (Item::Static(item_static), SymbolKind::Static) => item_static.ident == name,
            (Item::Type(item_type), SymbolKind::Type) => item_type.ident == name,
            (Item::Trait(item_trait), SymbolKind::Trait) => item_trait.ident == name,
            (Item::Mod(item_mod), SymbolKind::Module) => item_mod.ident == name,
            _ => false,
        };
        if matches {
            idx = Some(i);
            break;
        }
    }
    idx.map(|i| ast.items.remove(i))
}
fn rename_mod_decl(ast: &mut syn::File, old_name: &str, new_name: &str) -> bool {
    let mut changed = false;
    for item in &mut ast.items {
        if let Item::Mod(item_mod) = item {
            if item_mod.ident == old_name {
                item_mod.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
        }
    }
    changed
}
fn split_module_path(path: &str) -> Vec<String> {
    path.split("::").filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}
fn join_module_path(segments: &[String]) -> String {
    segments.join("::")
}
fn build_full_path(module_path: &str, name: &str) -> Vec<String> {
    let mut segments = split_module_path(module_path);
    segments.push(name.to_string());
    segments
}
fn symbol_kind_from_str(kind: &str) -> SymbolKind {
    match kind {
        "fn" => SymbolKind::Fn,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "const" => SymbolKind::Const,
        "static" => SymbolKind::Static,
        "type" => SymbolKind::Type,
        "trait" => SymbolKind::Trait,
        "module" => SymbolKind::Module,
        _ => SymbolKind::Fn,
    }
}
fn use_tree_group_prefix(
    tree: &mut syn::UseTree,
) -> Option<(Vec<String>, &mut syn::UseGroup)> {
    let mut prefix = Vec::new();
    let mut current = tree;
    loop {
        match current {
            syn::UseTree::Path(use_path) => {
                prefix.push(use_path.ident.to_string());
                current = &mut *use_path.tree;
            }
            syn::UseTree::Group(group) => return Some((prefix, group)),
            _ => return None,
        }
    }
}
struct PathRewriter {
    old_full: Option<Vec<String>>,
    new_full: Option<Vec<String>>,
    old_prefix: Option<Vec<String>>,
    new_prefix: Option<Vec<String>>,
    changed: bool,
}
impl PathRewriter {
    fn replace_full(old_full: &[String], new_full: &[String]) -> Self {
        Self {
            old_full: Some(old_full.to_vec()),
            new_full: Some(new_full.to_vec()),
            old_prefix: None,
            new_prefix: None,
            changed: false,
        }
    }
    fn replace_prefix(old_prefix: &[String], new_prefix: &[String]) -> Self {
        Self {
            old_full: None,
            new_full: None,
            old_prefix: Some(old_prefix.to_vec()),
            new_prefix: Some(new_prefix.to_vec()),
            changed: false,
        }
    }
    fn visit_file(&mut self, file: &mut syn::File) -> bool {
        self.changed = false;
        self.visit_file_mut(file);
        self.changed
    }
    fn rewrite_segments(&self, segments: &mut Vec<String>) -> bool {
        let original = segments.clone();
        if let (Some(old_full), Some(new_full)) = (&self.old_full, &self.new_full) {
            if segments == old_full {
                *segments = new_full.clone();
            } else if segments.starts_with(old_full) {
                let mut replaced = new_full.clone();
                replaced.extend_from_slice(&segments[old_full.len()..]);
                *segments = replaced;
            }
        }
        if let (Some(old_prefix), Some(new_prefix)) = (
            &self.old_prefix,
            &self.new_prefix,
        ) {
            if segments.starts_with(old_prefix) {
                let mut replaced = new_prefix.clone();
                replaced.extend_from_slice(&segments[old_prefix.len()..]);
                *segments = replaced;
            }
        }
        *segments != original
    }
}
impl VisitMut for PathRewriter {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        let mut segments: Vec<String> = path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        let local_changed = self.rewrite_segments(&mut segments);
        if local_changed {
            path.segments.clear();
            for seg in segments {
                path.segments
                    .push(syn::PathSegment {
                        ident: syn::Ident::new(&seg, Span::call_site()),
                        arguments: syn::PathArguments::None,
                    });
            }
            self.changed = true;
        }
        syn::visit_mut::visit_path_mut(self, path);
    }
    fn visit_use_tree_mut(&mut self, tree: &mut syn::UseTree) {
        if let Some((segments, tail)) = flatten_use_tree(tree) {
            let mut new_segments = segments.clone();
            let local_changed = self.rewrite_segments(&mut new_segments);
            if new_segments != segments {
                *tree = build_use_tree(&new_segments, tail);
                if local_changed {
                    self.changed = true;
                }
                return;
            }
        }
        if let Some((prefix, group)) = use_tree_group_prefix(tree) {
            let mut local_changed = false;
            for item in group.items.iter_mut() {
                if let Some((segments, tail)) = flatten_use_tree(item) {
                    let mut candidate = prefix.clone();
                    candidate.extend_from_slice(&segments);
                    let mut rewritten = candidate.clone();
                    if self.rewrite_segments(&mut rewritten) {
                        if rewritten.starts_with(&prefix) {
                            let rewritten_tail = &rewritten[prefix.len()..];
                            *item = build_use_tree(rewritten_tail, tail);
                            local_changed = true;
                        }
                    }
                }
            }
            if local_changed {
                self.changed = true;
                return;
            }
        }
        syn::visit_mut::visit_use_tree_mut(self, tree);
    }
}
struct IdentRewriter {
    old: String,
    new: String,
    changed: bool,
}
impl IdentRewriter {
    fn new(old: &str, new: &str) -> Self {
        Self {
            old: old.to_string(),
            new: new.to_string(),
            changed: false,
        }
    }
    fn visit_file(&mut self, file: &mut syn::File) -> bool {
        self.changed = false;
        self.visit_file_mut(file);
        self.changed
    }
}
impl VisitMut for IdentRewriter {
    fn visit_ident_mut(&mut self, i: &mut syn::Ident) {
        if i == &self.old {
            *i = syn::Ident::new(&self.new, Span::call_site());
            self.changed = true;
        }
        syn::visit_mut::visit_ident_mut(self, i);
    }
    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
        let (new_tokens, local_changed) =
            rewrite_tokens_ident(mac.tokens.clone(), &self.old, &self.new);
        if local_changed {
            mac.tokens = new_tokens;
            self.changed = true;
        }
        syn::visit_mut::visit_macro_mut(self, mac);
    }
}

fn rewrite_tokens_ident(
    stream: proc_macro2::TokenStream,
    old: &str,
    new: &str,
) -> (proc_macro2::TokenStream, bool) {
    let mut out = proc_macro2::TokenStream::new();
    let mut changed = false;
    for tt in stream.into_iter() {
        match tt {
            proc_macro2::TokenTree::Ident(ident) => {
                if ident.to_string() == old {
                    out.extend(std::iter::once(proc_macro2::TokenTree::Ident(
                        proc_macro2::Ident::new(new, ident.span()),
                    )));
                    changed = true;
                } else {
                    out.extend(std::iter::once(proc_macro2::TokenTree::Ident(ident)));
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                let (inner, inner_changed) =
                    rewrite_tokens_ident(group.stream(), old, new);
                if inner_changed {
                    let mut new_group =
                        proc_macro2::Group::new(group.delimiter(), inner);
                    new_group.set_span(group.span());
                    out.extend(std::iter::once(proc_macro2::TokenTree::Group(
                        new_group,
                    )));
                    changed = true;
                } else {
                    out.extend(std::iter::once(proc_macro2::TokenTree::Group(
                        group,
                    )));
                }
            }
            other => out.extend(std::iter::once(other)),
        }
    }
    (out, changed)
}
#[derive(Clone)]
enum UseTail {
    Name,
    Rename(syn::Ident),
}
fn flatten_use_tree(tree: &syn::UseTree) -> Option<(Vec<String>, UseTail)> {
    let mut segments = Vec::new();
    let mut current = tree;
    loop {
        match current {
            syn::UseTree::Path(path) => {
                segments.push(path.ident.to_string());
                current = &path.tree;
            }
            syn::UseTree::Name(name) => {
                segments.push(name.ident.to_string());
                return Some((segments, UseTail::Name));
            }
            syn::UseTree::Rename(rename) => {
                segments.push(rename.ident.to_string());
                return Some((segments, UseTail::Rename(rename.rename.clone())));
            }
            syn::UseTree::Glob(_) | syn::UseTree::Group(_) => return None,
        }
    }
}
fn file_has_use_of(file: &syn::File, target: &[String]) -> bool {
    for item in &file.items {
        if let Item::Use(item_use) = item {
            if use_tree_contains_path(&item_use.tree, target) {
                return true;
            }
        }
    }
    false
}
fn file_has_use_of_any(file: &syn::File, targets: &[&[String]]) -> bool {
    if targets.is_empty() {
        return false;
    }
    for item in &file.items {
        if let Item::Use(item_use) = item {
            if targets.iter().any(|t| use_tree_contains_path_or_suffix(&item_use.tree, t, &[])) {
                return true;
            }
        }
    }
    false
}
fn use_tree_contains_path(tree: &syn::UseTree, target: &[String]) -> bool {
    match tree {
        syn::UseTree::Group(group) => {
            group.items.iter().any(|t| use_tree_contains_path(t, target))
        }
        _ => {
            if let Some((segments, _)) = flatten_use_tree(tree) {
                segments == target
            } else {
                false
            }
        }
    }
}
fn use_tree_contains_path_or_suffix(
    tree: &syn::UseTree,
    target: &[String],
    prefix: &[String],
) -> bool {
    match tree {
        syn::UseTree::Path(path) => {
            let mut next_prefix = prefix.to_vec();
            next_prefix.push(path.ident.to_string());
            use_tree_contains_path_or_suffix(&path.tree, target, &next_prefix)
        }
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|t| use_tree_contains_path_or_suffix(t, target, prefix)),
        syn::UseTree::Name(name) => {
            let mut full = prefix.to_vec();
            full.push(name.ident.to_string());
            full == target
        }
        syn::UseTree::Rename(rename) => {
            let mut full = prefix.to_vec();
            full.push(rename.ident.to_string());
            full == target
        }
        _ => false,
    }
}
fn build_use_tree(segments: &[String], tail: UseTail) -> syn::UseTree {
    if segments.is_empty() {
        return syn::UseTree::Glob(syn::UseGlob {
            star_token: syn::token::Star::default(),
        });
    }
    if segments.len() == 1 {
        let ident = syn::Ident::new(&segments[0], Span::call_site());
        return match tail {
            UseTail::Name => syn::UseTree::Name(syn::UseName { ident }),
            UseTail::Rename(rename) => {
                syn::UseTree::Rename(syn::UseRename {
                    ident,
                    rename,
                    as_token: Default::default(),
                })
            }
        };
    }
    let ident = syn::Ident::new(&segments[0], Span::call_site());
    syn::UseTree::Path(syn::UsePath {
        ident,
        colon2_token: Default::default(),
        tree: Box::new(build_use_tree(&segments[1..], tail)),
    })
}
