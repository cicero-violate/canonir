#![cfg(feature = "rustc_frontend")]

use anyhow::{anyhow, Result};
use cargo::core::compiler::{CompileKind, CompileMode, DefaultExecutor, Executor, RustcTargetData, UserIntent};
use cargo::core::Verbosity;
use cargo::core::Workspace;
use cargo::ops::{compile_with_exec, CompileFilter, CompileOptions, FilterRule, LibRule};
use cargo::util::context::GlobalContext;
use cargo::util::CargoResult;
use cargo_util::ProcessBuilder;
use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::Res;
use rustc_hir::intravisit::{self, Visitor};
use rustc_interface::interface;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct SpanRange {
    pub lo: usize,
    pub hi: usize,
}

pub struct RustcResolver {
    project_root: PathBuf,
    source_root: PathBuf,
    crate_name: String,
}

impl RustcResolver {
    pub fn new(project_root: &Path) -> Result<Self> {
        let source_root = determine_source_root(project_root);
        let crate_name = infer_crate_name(project_root)?;
        Ok(Self { project_root: project_root.to_path_buf(), source_root, crate_name })
    }

    pub fn debug_cargo_rustc_args(&self) -> Result<Vec<String>> {
        cargo_rustc_args(&self.project_root, &self.source_root, &self.crate_name)
    }

    pub fn debug_def_paths(&self) -> Result<Vec<String>> {
        let input = self.source_root.join("lib.rs");
        if !input.exists() {
            return Err(anyhow!("expected lib.rs under {}", self.source_root.display()));
        }
        let mut callback = CollectorCallbacks::new("__debug_dump__".to_string());
        let mut args = cargo_rustc_args(&self.project_root, &self.source_root, &self.crate_name)?;
        // Reduce incremental + parallelism to avoid rustc dep-graph reentrancy panics.
        args.push("-Z".to_string());
        args.push("incremental-verify-ich=no".to_string());
        args.push("-Z".to_string());
        args.push("threads=1".to_string());
        run_rustc_in_dir(&self.project_root, &args, &mut callback);
        Ok(callback.into_def_paths())
    }

    pub fn collect_occurrences(&self, symbol_id: &str) -> Result<HashMap<PathBuf, Vec<SpanRange>>> {
        let input = self.source_root.join("lib.rs");
        if !input.exists() {
            return Err(anyhow!("expected lib.rs under {}", self.source_root.display()));
        }
        let mut callback = CollectorCallbacks::new(symbol_id.to_string());
        let args = cargo_rustc_args(&self.project_root, &self.source_root, &self.crate_name)?;
        run_rustc_in_dir(&self.project_root, &args, &mut callback);
        callback.into_result()
    }
}

fn determine_source_root(project: &Path) -> PathBuf {
    let src = project.join("src");
    if src.is_dir() {
        src
    } else {
        project.to_path_buf()
    }
}

fn infer_crate_name(project_root: &Path) -> Result<String> {
    let cargo_toml = project_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)?;
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("name") {
            if let Some(eq_idx) = rest.find('=') {
                let value = rest[eq_idx + 1..].trim().trim_matches('"');
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }
    Err(anyhow!("could not infer crate name from {}", cargo_toml.display()))
}

fn cargo_rustc_args(project_root: &Path, source_root: &Path, crate_name: &str) -> Result<Vec<String>> {
    let mut gctx = GlobalContext::default()?;
    gctx.shell().set_verbosity(Verbosity::Quiet);
    let manifest_path = project_root.join("Cargo.toml");
    let ws = Workspace::new(&manifest_path, &gctx)?;
    let mut options = CompileOptions::new(&gctx, UserIntent::Check { test: false })?;
    options.filter = CompileFilter::new(LibRule::True, FilterRule::none(), FilterRule::none(), FilterRule::none(), FilterRule::none());
    options.build_config.force_rebuild = true;
    let source_file = source_root.join("lib.rs");
    let capture = Arc::new(RustcArgsCapture::new(source_file, crate_name.to_string()));
    let exec: Arc<dyn Executor> = capture.clone();
    let _compilation = compile_with_exec(&ws, &options, &exec)?;
    let mut args = capture.take_args().ok_or_else(|| anyhow!("failed to capture rustc args for {}", crate_name))?;
    ensure_sysroot(&ws, &mut args)?;
    absolutize_input_paths(ws.root(), &mut args);
    Ok(args)
}

struct RustcArgsCapture {
    source_file: PathBuf,
    crate_name: String,
    args: Mutex<Option<Vec<String>>>,
    exec: DefaultExecutor,
}

impl RustcArgsCapture {
    fn new(source_file: PathBuf, crate_name: String) -> Self {
        Self { source_file, crate_name, args: Mutex::new(None), exec: DefaultExecutor }
    }

    fn take_args(&self) -> Option<Vec<String>> {
        self.args.lock().ok().and_then(|mut guard| guard.take())
    }

    fn should_capture(&self, target: &cargo::core::Target, mode: CompileMode) -> bool {
        if !target.is_lib() {
            return false;
        }
        if !matches!(mode, CompileMode::Check { test: false } | CompileMode::Build) {
            return false;
        }
        let normalized = self.crate_name.replace('-', "_");
        if target.name() != self.crate_name && target.name() != normalized {
            return false;
        }
        target.src_path().path() == Some(self.source_file.as_path())
    }

    fn record_args(&self, cmd: &ProcessBuilder) {
        let mut guard = match self.args.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if guard.is_some() {
            return;
        }
        *guard = Some(process_builder_to_rustc_args(cmd));
    }
}

impl Executor for RustcArgsCapture {
    fn exec(
        &self, cmd: &ProcessBuilder, id: cargo::core::PackageId, target: &cargo::core::Target, mode: CompileMode, on_stdout_line: &mut dyn FnMut(&str) -> CargoResult<()>,
        on_stderr_line: &mut dyn FnMut(&str) -> CargoResult<()>,
    ) -> CargoResult<()> {
        if self.should_capture(target, mode) {
            self.record_args(cmd);
        }
        self.exec.exec(cmd, id, target, mode, on_stdout_line, on_stderr_line)
    }
}

fn process_builder_to_rustc_args(cmd: &ProcessBuilder) -> Vec<String> {
    let mut raw_args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    if let Some(first) = raw_args.first().cloned() {
        if looks_like_rustc(&first) {
            let mut args = Vec::with_capacity(raw_args.len());
            args.push(first);
            args.extend(raw_args.into_iter().skip(1));
            return args;
        }
    }
    let mut args = Vec::with_capacity(raw_args.len() + 1);
    args.push(cmd.get_program().to_string_lossy().into_owned());
    args.append(&mut raw_args);
    args
}

fn looks_like_rustc(arg: &str) -> bool {
    let filename = Path::new(arg).file_name().and_then(OsStr::to_str);
    matches!(filename, Some("rustc") | Some("rustc.exe"))
}

fn ensure_sysroot(ws: &Workspace<'_>, args: &mut Vec<String>) -> Result<()> {
    if args.iter().any(|arg| arg == "--sysroot") {
        return Ok(());
    }
    let requested = [CompileKind::Host];
    let target_data = RustcTargetData::new(ws, &requested)?;
    let info = target_data.info(CompileKind::Host);
    args.push("--sysroot".to_string());
    args.push(info.sysroot.display().to_string());
    Ok(())
}

fn run_rustc_in_dir(callback_dir: &Path, args: &[String], callback: &mut CollectorCallbacks) {
    let prev_dir = std::env::current_dir().ok();
    let prev_incremental = std::env::var("CARGO_INCREMENTAL").ok();
    let prev_force_incremental = std::env::var("RUSTC_FORCE_INCREMENTAL").ok();
    std::env::set_var("CARGO_INCREMENTAL", "0");
    std::env::set_var("RUSTC_FORCE_INCREMENTAL", "0");

    if std::env::set_current_dir(callback_dir).is_ok() {
        rustc_driver::run_compiler(args, callback);
        if let Some(prev) = prev_dir {
            let _ = std::env::set_current_dir(prev);
        }
    } else {
        rustc_driver::run_compiler(args, callback);
    }

    if let Some(value) = prev_incremental {
        std::env::set_var("CARGO_INCREMENTAL", value);
    } else {
        std::env::remove_var("CARGO_INCREMENTAL");
    }
    if let Some(value) = prev_force_incremental {
        std::env::set_var("RUSTC_FORCE_INCREMENTAL", value);
    } else {
        std::env::remove_var("RUSTC_FORCE_INCREMENTAL");
    }
}

fn absolutize_input_paths(workspace_root: &Path, args: &mut Vec<String>) {
    for arg in args.iter_mut() {
        if arg.ends_with(".rs") && Path::new(arg).is_relative() {
            let abs = workspace_root.join(&*arg);
            *arg = abs.display().to_string();
            break;
        }
    }
}

struct CollectorCallbacks {
    target_symbol: String,
    occurrences: HashMap<PathBuf, Vec<SpanRange>>,
    errors: Vec<String>,
    def_paths: Vec<String>,
}

impl CollectorCallbacks {
    fn new(target_symbol: String) -> Self {
        Self { target_symbol, occurrences: HashMap::new(), errors: Vec::new(), def_paths: Vec::new() }
    }

    fn into_result(self) -> Result<HashMap<PathBuf, Vec<SpanRange>>> {
        if self.errors.is_empty() {
            Ok(self.occurrences)
        } else {
            Err(anyhow!(self.errors.join("\n")))
        }
    }

    fn into_def_paths(mut self) -> Vec<String> {
        self.def_paths.sort();
        self.def_paths.dedup();
        self.def_paths
    }

    fn record_span(&mut self, source_map: &SourceMap, span: Span) {
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
        let range = SpanRange { lo: lo.pos.0 as usize, hi: hi.pos.0 as usize };
        self.occurrences.entry(path).or_default().push(range);
    }
}

impl Callbacks for CollectorCallbacks {
    fn config(&mut self, config: &mut interface::Config) {
        config.opts.unstable_opts.incremental_ignore_spans = true;
        config.opts.unstable_opts.codegen_backend = Some("dummy".to_string());
    }

    fn after_analysis<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: rustc_middle::ty::TyCtxt<'tcx>) -> Compilation {
        if let Err(err) = self.collect(tcx) {
            self.errors.push(err.to_string());
        }
        Compilation::Stop
    }
}

impl CollectorCallbacks {
    fn collect<'tcx>(&mut self, tcx: rustc_middle::ty::TyCtxt<'tcx>) -> Result<()> {
        let source_map = tcx.sess.source_map();
        self.def_paths.clear();
        for def_id in tcx.hir_crate_items(()).definitions() {
            let path = tcx.def_path_str(def_id.to_def_id());
            self.def_paths.push(path);
        }
        let target_def = find_def_id(tcx, &self.target_symbol);
        let Some(target_def) = target_def else {
            return Err(anyhow!("symbol not found via rustc: {}", self.target_symbol));
        };
        let def_span = tcx.def_span(target_def);
        self.record_span(source_map, def_span);
        let mut visitor = PathVisitor { target_def, source_map, sink: self };
        tcx.hir_visit_all_item_likes_in_crate(&mut visitor);
        for def_id in tcx.hir_body_owners() {
            let body = tcx.hir_body_owned_by(def_id);
            visitor.visit_body(body);
        }
        Ok(())
    }
}

fn find_def_id<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, target: &str) -> Option<rustc_hir::def_id::DefId> {
    let normalized = target.strip_prefix("crate::").or_else(|| target.strip_prefix("self::")).or_else(|| target.strip_prefix("super::")).unwrap_or(target);
    for def_id in tcx.hir_crate_items(()).definitions() {
        let path = tcx.def_path_str(def_id.to_def_id());
        if path == normalized {
            return Some(def_id.to_def_id());
        }
    }
    None
}

struct PathVisitor<'a> {
    target_def: rustc_hir::def_id::DefId,
    source_map: &'a SourceMap,
    sink: &'a mut CollectorCallbacks,
}

impl<'a, 'v> Visitor<'v> for PathVisitor<'a> {
    fn visit_path(&mut self, path: &rustc_hir::Path<'_>, _id: rustc_hir::HirId) {
        if let Res::Def(_, def_id) = path.res {
            if def_id == self.target_def {
                if let Some(seg) = path.segments.last() {
                    self.sink.record_span(self.source_map, seg.ident.span);
                }
            }
        }
        intravisit::walk_path(self, path);
    }
}
