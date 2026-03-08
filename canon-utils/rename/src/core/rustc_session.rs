use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, Visitor};
use rustc_interface::interface;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct SpanRange {
    pub lo: usize,
    pub hi: usize,
}

pub struct RustcSession {
    span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: HashMap<String, String>,
    symbol_catalog: Vec<(String, String)>,
    pub normalized_sources: HashMap<PathBuf, String>,
}

impl RustcSession {
    pub fn build(project_root: &Path) -> Result<Self> {
        let source_root = crate::core::rustc_resolver::determine_source_root(project_root);
        let crate_name = crate::core::rustc_resolver::infer_crate_name(project_root)?;
        let mut all_rustc_args =
            crate::core::rustc_resolver::cargo_rustc_args(project_root, &source_root, &crate_name)?;
        for rustc_args in &mut all_rustc_args {
            if !rustc_args.iter().any(|arg| arg == "-Z") {
                let threads = num_cpus::get().max(1);
                rustc_args.push("-Z".to_string());
                rustc_args.push(format!("threads={threads}"));
            }
        }

        let out_path = span_output_path()?;
        // Truncate any previous output before appending per-target spans.
        let _ = File::create(&out_path)?;
        let mut status = Ok(());
        for rustc_args in &all_rustc_args {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&out_path)?;
            let writer = BufWriter::new(file);
            let mut callbacks = BulkCollectorCallbacks::new(writer, crate_name.clone());

            let pass_status = rustc_driver::catch_fatal_errors(|| {
                rustc_driver::run_compiler(rustc_args, &mut callbacks);
            });

            if status.is_ok() && pass_status.is_err() {
                status = Err(anyhow!("rustc_driver failed during span collection"));
            }

            callbacks.finalize()?;
        }

        let (mut span_index, symbol_kinds, normalized_sources, saw_done) =
            load_spans_from_file(&out_path)?;
        if symbol_kinds.is_empty() {
            return Err(anyhow!("span collector produced no output"));
        }

        if !saw_done {
            return Err(anyhow!("span collector did not finish writing spans"));
        }

        let mut symbol_catalog: Vec<(String, String)> = symbol_kinds
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        symbol_catalog.sort_by(|a, b| a.0.cmp(&b.0));

        for per_file in span_index.values_mut() {
            for spans in per_file.values_mut() {
                spans.sort_by(|a, b| a.lo.cmp(&b.lo));
                spans.dedup_by(|a, b| a.lo == b.lo && a.hi == b.hi);
            }
        }

        Ok(Self {
            span_index,
            symbol_kinds,
            symbol_catalog,
            normalized_sources,
        })
    }

    pub fn spans_for(&self, symbol_id: &str) -> Option<&HashMap<PathBuf, Vec<SpanRange>>> {
        self.span_index.get(symbol_id)
    }

    pub fn normalized_source(&self, path: &PathBuf) -> Option<&String> {
        self.normalized_sources.get(path)
    }

    pub fn symbol_catalog(&self) -> Vec<(String, String)> {
        self.symbol_catalog.clone()
    }

    pub fn symbol_ids(&self) -> Vec<String> {
        self.symbol_catalog
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn symbol_kind(&self, symbol_id: &str) -> Option<&str> {
        self.symbol_kinds
            .get(symbol_id)
            .map(|value| value.as_str())
    }
}

struct BulkCollectorCallbacks {
    def_id_to_symbol: HashMap<rustc_hir::def_id::DefId, String>,
    symbol_kinds: HashMap<String, String>,
    out: BufWriter<File>,
    span_count: usize,
    emitted_source_files: std::collections::HashSet<PathBuf>,
    crate_name: String,
}

impl BulkCollectorCallbacks {
    fn new(out: BufWriter<File>, crate_name: String) -> Self {
        Self {
            def_id_to_symbol: HashMap::new(),
            symbol_kinds: HashMap::new(),
            out,
            span_count: 0,
            emitted_source_files: std::collections::HashSet::new(),
            crate_name,
        }
    }

    fn finalize(&mut self) -> Result<()> {
        let line = json!({
            "type": "done",
            "symbol_count": self.symbol_kinds.len(),
            "span_count": self.span_count
        })
        .to_string();
        writeln!(self.out, "{line}")?;
        self.out.flush()?;
        Ok(())
    }

    fn emit_span(&mut self, symbol_id: &str, source_map: &SourceMap, span: Span) {
        if span.from_expansion() {
            return;
        }
        let lo = source_map.lookup_byte_offset(span.lo());
        let hi = source_map.lookup_byte_offset(span.hi());
        if !Arc::ptr_eq(&lo.sf, &hi.sf) {
            return;
        }
        let filename = &lo.sf.name;
        let FileName::Real(real_path) = filename else { return };
        let Some(path) = real_path.local_path().map(|p| p.to_path_buf()) else { return };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let kind = self
            .symbol_kinds
            .get(symbol_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let lo_pos = lo.pos.0 as usize;
        let hi_pos = hi.pos.0 as usize;

        // Emit the normalized source text once per file so the patcher can use
        // it instead of re-reading from disk (disk bytes may differ due to
        // CRLF normalization or BOM removal that rustc performs on load).
        if self.emitted_source_files.insert(path.clone()) {
            if let Some(src) = lo.sf.src.as_deref() {
                let src_line = json!({
                    "type": "source",
                    "file": path.display().to_string(),
                    "src": src
                })
                .to_string();
                let _ = writeln!(self.out, "{src_line}");
            }
        }

        let line = json!({
            "symbol_id": symbol_id,
            "kind": kind,
            "file": path.display().to_string(),
            "lo": lo_pos,
            "hi": hi_pos
        })
        .to_string();
        if writeln!(self.out, "{line}").is_ok() {
            self.span_count += 1;
        }
    }
}

impl Callbacks for BulkCollectorCallbacks {
    fn config(&mut self, _config: &mut interface::Config) {}

    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> Compilation {
        let source_map = tcx.sess.source_map();
        self.def_id_to_symbol.clear();

        let idx = canon_capture::index::build_index(tcx);
        for def_id in idx.def_ids {
            let path = tcx.def_path_str(def_id);
            let symbol_id = if path == "crate" {
                "crate".to_string()
            } else {
                format!("crate::{path}")
            };
            let kind = def_kind_to_symbol_kind(tcx.def_kind(def_id));
            self.symbol_kinds
                .entry(symbol_id.clone())
                .or_insert_with(|| kind.to_string());
            self.def_id_to_symbol.insert(def_id, symbol_id.clone());
        }

        let mut visitor = PathVisitor {
            source_map,
            sink: self,
            tcx,
        };
        tcx.hir_visit_all_item_likes_in_crate(&mut visitor);
        Compilation::Stop
    }
}

fn def_kind_to_symbol_kind(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Fn | DefKind::AssocFn => "fn",
        DefKind::Struct => "struct",
        DefKind::Enum => "enum",
        DefKind::Const | DefKind::AssocConst => "const",
        DefKind::Static { .. } => "static",
        DefKind::TyAlias | DefKind::AssocTy => "type",
        DefKind::Trait => "trait",
        DefKind::Mod => "module",
        _ => "unknown",
    }
}

fn def_id_symbol_for_item(
    sink: &mut BulkCollectorCallbacks,
    item: &rustc_hir::Item<'_>,
) -> Option<String> {
    let def_id = item.owner_id.to_def_id();
    sink.def_id_to_symbol.get(&def_id).cloned()
}

fn item_ident_span(item: &rustc_hir::Item<'_>) -> Option<Span> {
    item.kind.ident().map(|ident| ident.span)
}

struct PathVisitor<'sm, 'cb, 'v> {
    source_map: &'sm SourceMap,
    sink: &'cb mut BulkCollectorCallbacks,
    tcx: rustc_middle::ty::TyCtxt<'v>,
}

impl<'sm, 'cb, 'v> Visitor<'v> for PathVisitor<'sm, 'cb, 'v> {
    type NestedFilter = rustc_middle::hir::nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_item(&mut self, item: &'v rustc_hir::Item<'v>) {
        if item.span.from_expansion() {
            return;
        }
        if let Some(symbol_id) = def_id_symbol_for_item(&mut *self.sink, item) {
            if let Some(ident_span) = item_ident_span(item) {
                self.sink.emit_span(&symbol_id, self.source_map, ident_span);
            }
        }
        if let rustc_hir::ItemKind::Use(path, use_kind) = &item.kind {
            match use_kind {
                rustc_hir::UseKind::Single(ident) => {
                    for res in path.res.present_items() {
                        if let Res::Def(_, def_id) = res {
                            if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                                self.sink
                                    .emit_span(&symbol_id, self.source_map, ident.span);
                            }
                        }
                    }
                }
                rustc_hir::UseKind::Glob => {
                    if let Some(seg) = path.segments.last() {
                        for res in path.res.present_items() {
                            if let Res::Def(_, def_id) = res {
                                if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                                    self.sink
                                        .emit_span(&symbol_id, self.source_map, seg.ident.span);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        intravisit::walk_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'v rustc_hir::ImplItem<'v>) {
        if item.span.from_expansion() {
            return;
        }
        let def_id = item.owner_id.to_def_id();
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            self.sink
                .emit_span(&symbol_id, self.source_map, item.ident.span);
        }
        intravisit::walk_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'v rustc_hir::TraitItem<'v>) {
        if item.span.from_expansion() {
            return;
        }
        let def_id = item.owner_id.to_def_id();
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            self.sink
                .emit_span(&symbol_id, self.source_map, item.ident.span);
        }
        intravisit::walk_trait_item(self, item);
    }

    fn visit_ty(&mut self, ty: &'v rustc_hir::Ty<'v, rustc_hir::AmbigArg>) {
        if let rustc_hir::TyKind::Path(qpath) = &ty.kind {
            self.emit_qpath_span(qpath);
        }
        intravisit::walk_ty(self, ty);
    }

    fn visit_expr(&mut self, expr: &'v rustc_hir::Expr<'v>) {
        match &expr.kind {
            rustc_hir::ExprKind::Struct(qpath, ..) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::ExprKind::Path(qpath) => {
                match qpath {
                    rustc_hir::QPath::TypeRelative(_, segment) => {
                        let hir_id = expr.hir_id;
                        let res = self
                            .tcx
                            .typeck(hir_id.owner.def_id)
                            .qpath_res(qpath, hir_id);
                        if let Res::Def(_, def_id) = res {
                            if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                                self.sink.emit_span(
                                    &symbol_id,
                                    self.source_map,
                                    segment.ident.span,
                                );
                            }
                        }
                    }
                    _ => {
                        self.emit_qpath_span(qpath);
                    }
                }
            }
            rustc_hir::ExprKind::MethodCall(segment, _receiver, _, _) => {
                let hir_id = expr.hir_id;
                if let Some(def_id) = self
                    .tcx
                    .typeck(hir_id.owner.def_id)
                    .type_dependent_def_id(hir_id)
                {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        self.sink
                            .emit_span(&symbol_id, self.source_map, segment.ident.span);
                    }
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, expr);
    }

    fn visit_use(
        &mut self,
        path: &'v rustc_hir::UsePath<'v>,
        hir_id: rustc_hir::HirId,
    ) {
        intravisit::walk_use(self, path, hir_id);
    }

    fn visit_variant(&mut self, v: &'v rustc_hir::Variant<'v>) {
        if v.span.from_expansion() {
            return;
        }
        let def_id = v.def_id.to_def_id();
        if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
            self.sink
                .emit_span(&symbol_id, self.source_map, v.ident.span);
        }
        intravisit::walk_variant(self, v);
    }

    fn visit_pat(&mut self, pat: &'v rustc_hir::Pat<'v>) {
        match &pat.kind {
            rustc_hir::PatKind::TupleStruct(qpath, _, _) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::PatKind::Struct(qpath, _, _) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::PatKind::Expr(pat_expr) => {
                if let rustc_hir::PatExprKind::Path(qpath) = pat_expr.kind {
                    self.emit_qpath_span(&qpath);
                }
            }
            _ => {}
        }
        intravisit::walk_pat(self, pat);
    }
}

impl<'sm, 'cb, 'v> PathVisitor<'sm, 'cb, 'v> {
    fn symbol_id_for_def(&mut self, def_id: rustc_hir::def_id::DefId) -> Option<String> {
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            return Some(symbol_id);
        }
        let path = self.tcx.def_path_str(def_id);
        let local_path = if def_id.is_local() {
            Some(path.as_str())
        } else {
            let crate_name = self.sink.crate_name.as_str();
            let normalized = crate_name.replace('-', "_");
            let prefix = format!("{crate_name}::");
            let prefix_norm = format!("{normalized}::");
            if path == crate_name || path == normalized {
                Some("crate")
            } else if path.starts_with(&prefix) {
                Some(&path[prefix.len()..])
            } else if path.starts_with(&prefix_norm) {
                Some(&path[prefix_norm.len()..])
            } else {
                None
            }
        };
        let Some(local_path) = local_path else { return None };
        let symbol_id = if local_path == "crate" {
            "crate".to_string()
        } else {
            format!("crate::{local_path}")
        };
        let kind = def_kind_to_symbol_kind(self.tcx.def_kind(def_id));
        self.sink
            .symbol_kinds
            .entry(symbol_id.clone())
            .or_insert_with(|| kind.to_string());
        self.sink.def_id_to_symbol.insert(def_id, symbol_id.clone());
        Some(symbol_id)
    }

    fn emit_qpath_span(&mut self, qpath: &rustc_hir::QPath<'_>) {
        match qpath {
            rustc_hir::QPath::Resolved(_, path) => {
                if let Res::Def(_, def_id) = path.res {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        let short = symbol_id.rsplit("::").next().unwrap_or("");
                        for seg in path.segments.iter() {
                            if seg.ident.as_str() == short {
                                self.sink
                                    .emit_span(&symbol_id, self.source_map, seg.ident.span);
                            }
                        }
                    }
                    let def_kind = self.tcx.def_kind(def_id);
                    let mut enum_def_id = None;
                    match def_kind {
                        rustc_hir::def::DefKind::Variant => {
                            enum_def_id = Some(self.tcx.parent(def_id));
                        }
                        rustc_hir::def::DefKind::Ctor(_, _) => {
                            let variant_def_id = self.tcx.parent(def_id);
                            if self.tcx.def_kind(variant_def_id)
                                == rustc_hir::def::DefKind::Variant
                            {
                                enum_def_id = Some(self.tcx.parent(variant_def_id));
                            }
                        }
                        _ => {}
                    }
                    if let Some(enum_def_id) = enum_def_id {
                        if let Some(enum_symbol_id) = self.symbol_id_for_def(enum_def_id) {
                            let short = enum_symbol_id.rsplit("::").next().unwrap_or("");
                            if path.segments.len() >= 2 {
                                let seg = &path.segments[path.segments.len() - 2];
                                if seg.ident.as_str() == short {
                                    self.sink
                                        .emit_span(&enum_symbol_id, self.source_map, seg.ident.span);
                                }
                            }
                        }
                    }
                }
            }
            rustc_hir::QPath::TypeRelative(_, segment) => {
                if let Res::Def(_, def_id) = segment.res {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        self.sink
                            .emit_span(&symbol_id, self.source_map, segment.ident.span);
                    }
                }
            }
            _ => {}
        }
    }
}

fn span_output_path() -> Result<PathBuf> {
    let dir = PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/rename/span_file");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("spans.jsonl"))
}

fn load_spans_from_file(
    path: &Path,
) -> Result<(
    HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    HashMap<String, String>,
    HashMap<PathBuf, String>,
    bool,
)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>> = HashMap::new();
    let mut symbol_kinds: HashMap<String, String> = HashMap::new();
    let mut normalized_sources: HashMap<PathBuf, String> = HashMap::new();
    let mut saw_done = false;

    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => {
                line.clear();
                continue;
            }
        };
        if let Some(kind) = value.get("type").and_then(|v| v.as_str()) {
            if kind == "done" {
                saw_done = true;
            } else if kind == "source" {
                if let (Some(file), Some(src)) = (
                    value.get("file").and_then(|v| v.as_str()),
                    value.get("src").and_then(|v| v.as_str()),
                ) {
                    normalized_sources.insert(PathBuf::from(file), src.to_string());
                }
            }
            line.clear();
            continue;
        }
        let symbol_id = match value.get("symbol_id").and_then(|v| v.as_str()) {
            Some(value) => value,
            None => {
                line.clear();
                continue;
            }
        };
        let kind = value
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let file = match value.get("file").and_then(|v| v.as_str()) {
            Some(value) => value,
            None => {
                line.clear();
                continue;
            }
        };
        let lo = value.get("lo").and_then(|v| v.as_u64()).unwrap_or(0);
        let hi = value.get("hi").and_then(|v| v.as_u64()).unwrap_or(0);

        symbol_kinds
            .entry(symbol_id.to_string())
            .or_insert_with(|| kind.to_string());
        span_index
            .entry(symbol_id.to_string())
            .or_default()
            .entry(PathBuf::from(file))
            .or_default()
            .push(SpanRange {
                lo: lo as usize,
                hi: hi as usize,
            });

        line.clear();
    }

    Ok((span_index, symbol_kinds, normalized_sources, saw_done))
}
