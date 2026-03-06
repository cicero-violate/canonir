use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
}

impl RustcSession {
    pub fn build(project_root: &Path) -> Result<Self> {
        let source_root = crate::core::rustc_resolver::determine_source_root(project_root);
        let crate_name = crate::core::rustc_resolver::infer_crate_name(project_root)?;
        let mut rustc_args =
            crate::core::rustc_resolver::cargo_rustc_args(project_root, &source_root, &crate_name)?;
        if !rustc_args.iter().any(|arg| arg == "-Z") {
            let threads = num_cpus::get().max(1);
            rustc_args.push("-Z".to_string());
            rustc_args.push(format!("threads={threads}"));
        }

        let out_path = span_output_path()?;
        let shared = Arc::new(Mutex::new(CollectorOutput::default()));
        let file = File::create(&out_path)?;
        let writer = BufWriter::new(file);
        let mut callbacks = BulkCollectorCallbacks::new(shared.clone(), writer)?;

        let status = rustc_driver::catch_fatal_errors(|| {
            rustc_driver::run_compiler(&rustc_args, &mut callbacks);
        });

        callbacks.finalize()?;

        let output = shared.lock().map_err(|_| anyhow!("span collector lock poisoned"))?;
        if output.symbol_kinds.is_empty() {
            if status.is_err() {
                return Err(anyhow!("rustc_driver failed during span collection"));
            }
            return Err(anyhow!("span collector produced no output"));
        }

        let mut span_index = output.span_index.clone();
        let mut symbol_kinds = output.symbol_kinds.clone();

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
        })
    }

    pub fn spans_for(&self, symbol_id: &str) -> Option<&HashMap<PathBuf, Vec<SpanRange>>> {
        self.span_index.get(symbol_id)
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

#[derive(Default)]
struct CollectorOutput {
    span_index: HashMap<String, HashMap<PathBuf, Vec<SpanRange>>>,
    symbol_kinds: HashMap<String, String>,
}

struct BulkCollectorCallbacks {
    shared: Arc<Mutex<CollectorOutput>>,
    def_id_to_symbol: HashMap<rustc_hir::def_id::DefId, String>,
    out: BufWriter<File>,
    span_count: usize,
}

impl BulkCollectorCallbacks {
    fn new(shared: Arc<Mutex<CollectorOutput>>, out: BufWriter<File>) -> Result<Self> {
        Ok(Self {
            shared,
            def_id_to_symbol: HashMap::new(),
            out,
            span_count: 0,
        })
    }

    fn finalize(&mut self) -> Result<()> {
        let line = json!({
            "type": "done",
            "symbol_count": self
                .shared
                .lock()
                .map_err(|_| anyhow!("span collector lock poisoned"))?
                .symbol_kinds
                .len(),
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
        let kind = self
            .shared
            .lock()
            .ok()
            .and_then(|output| output.symbol_kinds.get(symbol_id).cloned())
            .unwrap_or_else(|| "unknown".to_string());

        if let Ok(mut output) = self.shared.lock() {
            output
                .span_index
                .entry(symbol_id.to_string())
                .or_default()
                .entry(path.clone())
                .or_default()
                .push(SpanRange {
                    lo: lo.pos.0 as usize,
                    hi: hi.pos.0 as usize,
                });
        }

        let line = json!({
            "symbol_id": symbol_id,
            "kind": kind,
            "file": path.display().to_string(),
            "lo": lo.pos.0,
            "hi": hi.pos.0
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
            if let Ok(mut output) = self.shared.lock() {
                output
                    .symbol_kinds
                    .entry(symbol_id.clone())
                    .or_insert_with(|| kind.to_string());
            }
            self.def_id_to_symbol.insert(def_id, symbol_id.clone());
            let def_span = tcx.def_span(def_id);
            self.emit_span(&symbol_id, source_map, def_span);
        }

        let mut visitor = PathVisitor {
            source_map,
            sink: self,
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

struct PathVisitor<'sm, 'cb> {
    source_map: &'sm SourceMap,
    sink: &'cb mut BulkCollectorCallbacks,
}

impl<'sm, 'cb, 'v> Visitor<'v> for PathVisitor<'sm, 'cb> {
    fn visit_item(&mut self, item: &'v rustc_hir::Item<'v>) {
        if item.span.from_expansion() {
            return;
        }
        intravisit::walk_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'v rustc_hir::ImplItem<'v>) {
        if item.span.from_expansion() {
            return;
        }
        intravisit::walk_impl_item(self, item);
    }

    fn visit_path(&mut self, path: &rustc_hir::Path<'_>, _id: rustc_hir::HirId) {
        if let Res::Def(_, def_id) = path.res {
            if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
                if let Some(seg) = path.segments.last() {
                    self.sink
                        .emit_span(&symbol_id, self.source_map, seg.ident.span);
                }
            }
        }
        intravisit::walk_path(self, path);
    }

    fn visit_use(
        &mut self,
        path: &'v rustc_hir::UsePath<'v>,
        hir_id: rustc_hir::HirId,
    ) {
        let rustc_hir::Path { segments, res, .. } = *path;
        for res in res.present_items() {
            if let Res::Def(_, def_id) = res {
                if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
                    if let Some(seg) = segments.last() {
                        self.sink
                            .emit_span(&symbol_id, self.source_map, seg.ident.span);
                    }
                }
            }
        }
        intravisit::walk_use(self, path, hir_id);
    }
}

fn span_output_path() -> Result<PathBuf> {
    let dir = PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/rename/span_file");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("spans.jsonl"))
}
