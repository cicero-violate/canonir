use anyhow::Result;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::ty::TyCtxt;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn collect_spans_and_symbols(
    tcx: TyCtxt<'_>,
    _output_dir: &Path,
    crate_name: &str,
) -> Result<SymbolSpanBundle> {
    let writer: BufWriter<Box<dyn Write>> = BufWriter::new(Box::new(std::io::sink()));
    let mut collector = SpanCollector::new(writer, crate_name.to_string(), HashSet::new());
    collector.collect(tcx)?;
    collector.finalize()?;
    Ok(SymbolSpanBundle {
        spans_by_symbol: collector.span_map,
        kinds: collector.symbol_kinds,
    })
}

#[derive(Debug, Clone)]
pub struct SpanInfo {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub lo: u32,
    pub hi: u32,
}

#[derive(Debug, Clone)]
pub struct SymbolSpanBundle {
    pub spans_by_symbol: HashMap<String, Vec<SpanInfo>>,
    pub kinds: HashMap<String, String>,
}

pub fn collect_symbol_spans(
    tcx: TyCtxt<'_>,
) -> HashMap<String, Vec<SpanInfo>> {
    let source_map = tcx.sess.source_map();
    let idx = crate::capture::index::build_index(tcx);
    let mut out: HashMap<String, Vec<SpanInfo>> = HashMap::new();

    for def_id in idx.def_ids {
        let def_path = tcx.def_path_str(def_id);
        let short = def_path
            .rsplit("::")
            .next()
            .unwrap_or("")
            .to_string();
        if short.is_empty() {
            continue;
        }
        let span = tcx.def_span(def_id);
        if span.from_expansion() {
            continue;
        }
        let lo = source_map.lookup_byte_offset(span.lo());
        let hi = source_map.lookup_byte_offset(span.hi());
        if !Arc::ptr_eq(&lo.sf, &hi.sf) {
            continue;
        }
        let filename = &lo.sf.name;
        let FileName::Real(real_path) = filename else { continue };
        let Some(path) = real_path.local_path().map(|p| p.to_path_buf()) else { continue };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let file = path.display().to_string();
        let line_info = source_map.lookup_char_pos(span.lo());
        let line = line_info.line as u32;
        let col = line_info.col.0 as u32;
        let lo_pos = lo.pos.0 as u32;
        let hi_pos = hi.pos.0 as u32;

        let info = SpanInfo {
            file,
            line,
            col,
            lo: lo_pos,
            hi: hi_pos,
        };

        out.entry(def_path.clone()).or_default().push(info.clone());
        out.entry(short).or_default().push(info);
    }

    out
}

struct SpanCollector {
    def_id_to_symbol: HashMap<rustc_hir::def_id::DefId, String>,
    symbol_kinds: HashMap<String, String>,
    out: BufWriter<Box<dyn Write>>,
    span_count: usize,
    emitted_source_files: HashSet<PathBuf>,
    crate_name: String,
    span_map: HashMap<String, Vec<SpanInfo>>,
}

impl SpanCollector {
    fn new(
        out: BufWriter<Box<dyn Write>>,
        crate_name: String,
        emitted_source_files: HashSet<PathBuf>,
    ) -> Self {
        Self {
            def_id_to_symbol: HashMap::new(),
            symbol_kinds: HashMap::new(),
            out,
            span_count: 0,
            emitted_source_files,
            crate_name,
            span_map: HashMap::new(),
        }
    }

    fn collect(&mut self, tcx: TyCtxt<'_>) -> Result<()> {
        let source_map = tcx.sess.source_map();
        self.def_id_to_symbol.clear();

        let idx = crate::capture::index::build_index(tcx);
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

        // Always include definition spans so renames touch the defining item.
        let def_items: Vec<_> = self
            .def_id_to_symbol
            .iter()
            .map(|(def_id, symbol_id)| (*def_id, symbol_id.clone()))
            .collect();
        for (def_id, symbol_id) in def_items {
            let span = tcx.def_span(def_id);
            self.emit_def_span(&symbol_id, source_map, span);
        }
        Ok(())
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

        let info = SpanInfo {
            file: path.display().to_string(),
            line: source_map.lookup_char_pos(span.lo()).line as u32,
            col: source_map.lookup_char_pos(span.lo()).col.0 as u32,
            lo: lo_pos as u32,
            hi: hi_pos as u32,
        };
        self.span_map
            .entry(symbol_id.to_string())
            .or_default()
            .push(info.clone());
        if let Some(short) = symbol_id.rsplit("::").next() {
            if !short.is_empty() {
                self.span_map.entry(short.to_string()).or_default().push(info);
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

    fn emit_def_span(&mut self, symbol_id: &str, source_map: &SourceMap, span: Span) {
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

fn def_kind_to_symbol_kind(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Fn | DefKind::AssocFn => "fn",
        DefKind::Struct => "struct",
        DefKind::Enum => "enum",
        DefKind::Const { .. } | DefKind::AssocConst { .. } => "const",
        DefKind::Static { .. } => "static",
        DefKind::TyAlias | DefKind::AssocTy => "type",
        DefKind::Trait => "trait",
        DefKind::Mod => "module",
        _ => "unknown",
    }
}

fn def_id_symbol_for_item(
    sink: &mut SpanCollector,
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
    sink: &'cb mut SpanCollector,
    tcx: TyCtxt<'v>,
}

impl<'sm, 'cb, 'v> Visitor<'v> for PathVisitor<'sm, 'cb, 'v> {
    type NestedFilter = rustc_middle::hir::nested_filter::All;

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
            rustc_hir::ExprKind::Path(qpath) => match qpath {
                rustc_hir::QPath::TypeRelative(_, segment) => {
                    let hir_id = expr.hir_id;
                    let res = self
                        .tcx
                        .typeck(hir_id.owner.def_id)
                        .qpath_res(qpath, hir_id);
                    if let Res::Def(_, def_id) = res {
                        if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                            self.sink
                                .emit_span(&symbol_id, self.source_map, segment.ident.span);
                        }
                    }
                }
                _ => {
                    self.emit_qpath_span(qpath);
                }
            },
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
        }
    }
}
