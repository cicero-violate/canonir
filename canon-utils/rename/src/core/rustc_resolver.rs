use anyhow::{anyhow, Result};
use cargo::core::compiler::{CompileKind, CompileMode, DefaultExecutor, Executor, RustcTargetData, UserIntent};
use cargo::core::Verbosity;
use cargo::core::Workspace;
use cargo::ops::{compile_with_exec, CompileFilter, CompileOptions, FilterRule, LibRule};
use cargo::util::context::GlobalContext;
use cargo::util::CargoResult;
use cargo_util::ProcessBuilder;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) fn determine_source_root(project: &Path) -> PathBuf {
    let src = project.join("src");
    if src.is_dir() { src } else { project.to_path_buf() }
}

pub(crate) fn infer_crate_name(project_root: &Path) -> Result<String> {
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

pub(crate) fn cargo_rustc_args(
    project_root: &Path,
    source_root: &Path,
    crate_name: &str,
) -> Result<Vec<Vec<String>>> {
    let mut gctx = GlobalContext::default()?;
    gctx.shell().set_verbosity(Verbosity::Quiet);
    let manifest_path = project_root.join("Cargo.toml");
    let ws = Workspace::new(&manifest_path, &gctx)?;
    let mut options = CompileOptions::new(&gctx, UserIntent::Check { test: false })?;
    options.filter = CompileFilter::new(
        LibRule::True,
        FilterRule::All,
        FilterRule::none(),
        FilterRule::none(),
        FilterRule::none(),
    );
    options.build_config.force_rebuild = true;
    let lib_file = source_root.join("lib.rs");
    let bin_file = source_root.join("main.rs");
    let mut source_files = vec![lib_file];
    if bin_file.exists() {
        source_files.push(bin_file);
    }
    let capture = std::sync::Arc::new(RustcArgsCapture::new(source_files, crate_name.to_string()));
    let exec: std::sync::Arc<dyn Executor> = capture.clone();
    let _compilation = compile_with_exec(&ws, &options, &exec)?;
    let mut all_args =
        capture.take_args().ok_or_else(|| anyhow!("failed to capture rustc args for {}", crate_name))?;
    for args in &mut all_args {
        ensure_sysroot(&ws, args)?;
        absolutize_input_paths(ws.root(), args);
    }
    Ok(all_args)
}

struct RustcArgsCapture {
    source_files: Vec<PathBuf>,
    crate_name: String,
    args: Mutex<Vec<Vec<String>>>,
    exec: DefaultExecutor,
}

impl RustcArgsCapture {
    fn new(source_files: Vec<PathBuf>, crate_name: String) -> Self {
        Self { source_files, crate_name, args: Mutex::new(Vec::new()), exec: DefaultExecutor }
    }

    fn take_args(&self) -> Option<Vec<Vec<String>>> {
        self.args.lock().ok().map(|mut guard| {
            if guard.is_empty() {
                None
            } else {
                Some(std::mem::take(&mut *guard))
            }
        })?
    }

    fn should_capture(&self, target: &cargo::core::Target, mode: CompileMode) -> bool {
        if !target.is_lib() && !target.is_bin() {
            return false;
        }
        if !matches!(mode, CompileMode::Check { test: false } | CompileMode::Build) {
            return false;
        }
        let normalized = self.crate_name.replace('-', "_");
        if target.name() != self.crate_name && target.name() != normalized {
            return false;
        }
        self.source_files
            .iter()
            .any(|path| target.src_path().path() == Some(path.as_path()))
    }

    fn record_args(&self, cmd: &ProcessBuilder) {
        let mut guard = match self.args.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.push(process_builder_to_rustc_args(cmd));
    }
}

impl Executor for RustcArgsCapture {
    fn exec(
        &self,
        cmd: &ProcessBuilder,
        id: cargo::core::PackageId,
        target: &cargo::core::Target,
        mode: CompileMode,
        on_stdout_line: &mut dyn FnMut(&str) -> CargoResult<()>,
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

fn absolutize_input_paths(workspace_root: &Path, args: &mut Vec<String>) {
    for arg in args.iter_mut() {
        if arg.ends_with(".rs") && Path::new(arg).is_relative() {
            let abs = workspace_root.join(&*arg);
            *arg = abs.display().to_string();
            break;
        }
    }
}
