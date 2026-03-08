#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate upg_analysis;
extern crate serde_json;
extern crate libc;

use upg_analysis::{extract_and_write, OutputConfig};
use rustc_driver::Callbacks;
use rustc_session::EarlyDiagCtxt;
use rustc_errors::ColorConfig;
use rustc_errors::emitter::HumanReadableErrorType;
use rustc_session::config::ErrorOutputType;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

struct MirCaptureCallbacks {
    output_dir: PathBuf,
}

impl Callbacks for MirCaptureCallbacks {
    fn config(&mut self, config: &mut rustc_interface::Config) {
        let json_rendered = HumanReadableErrorType { short: false, unicode: false };
        config.opts.error_format = ErrorOutputType::Json {
            pretty: false,
            json_rendered,
            color_config: ColorConfig::Never,
        };
    }

    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        let config = OutputConfig {
            output_dir: self.output_dir.clone(),
        };
        if let Err(err) = extract_and_write(tcx, &config) {
            eprintln!("analysis_capture: extraction failed: {err:?}");
        }
        rustc_driver::Compilation::Continue
    }
}

fn exec_real_rustc(real_rustc: &str, args: &[String], reason: &str) -> ! {
    let status = std::process::Command::new(real_rustc)
        .args(args)
        .status()
        .unwrap_or_else(|err| panic!("failed to exec real rustc ({reason}): {err:?}"));
    std::process::exit(status.code().unwrap_or(0));
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let real_rustc = argv.get(1).cloned().expect("missing real rustc path");
    let crate_name = find_flag_value(&argv, "--crate-name");
    let crate_types = find_flag_values(&argv, "--crate-type");
    let is_probe = argv.iter().any(|a| a.starts_with("--print="))
        || argv.iter().any(|a| a == "-")
        || argv
            .windows(2)
            .any(|w| w[0] == "--crate-name" && w[1] == "___")
        || argv.iter().any(|a| a == "-vV" || a == "--version");

    if is_probe {
        exec_real_rustc(&real_rustc, &argv[2..], "probe");
    }

    let output_dir = project_root_from_env()
        .or_else(|| project_root_from_out_dir(&argv))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("analysis");

    if let Err(err) = fs::create_dir_all(&output_dir) {
        eprintln!("analysis_capture: failed to create output dir {output_dir:?}: {err}");
        exec_real_rustc(&real_rustc, &argv[2..], "output_dir");
    }

    let args: Vec<String> = std::iter::once(argv[0].clone())
        .chain(argv.iter().skip(2).cloned())
        .collect();

    let mut callbacks = MirCaptureCallbacks {
        output_dir: output_dir.clone(),
    };

    let _diag = EarlyDiagCtxt::new(rustc_session::config::ErrorOutputType::default());
    let errors_jsonl = output_dir.join("errors.jsonl");
    let errors_json = output_dir.join("errors.json");
    let result = with_stderr_redirect(&errors_jsonl, || {
        rustc_driver::catch_fatal_errors(|| {
            rustc_driver::run_compiler(&args, &mut callbacks);
        })
    });

    if let Ok(result) = result {
        let parse_result = parse_errors_jsonl(&errors_jsonl, &errors_json);
        let parse_ok = parse_result.is_ok();
        let _ = fs::remove_file(&errors_jsonl);
        if result.is_err() {
            if let Ok(summary) = parse_result {
                eprintln!(
                    "analysis_capture: rustc failed; {} errors at {}",
                    summary.count,
                    errors_json.display()
                );
                if !summary.by_code.is_empty() {
                    eprintln!("analysis_capture: error categories");
                    let mut items: Vec<_> = summary.by_code.into_iter().collect();
                    items.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));
                    let code_w = items.iter().map(|(c, _)| c.len()).max().unwrap_or(4);
                    let level_w = items.iter().map(|(_, e)| e.level.len()).max().unwrap_or(5);
                    let desc_w = items.iter().map(|(_, e)| e.message.len()).max().unwrap_or(11);
                    let count_w = items.iter().map(|(_, e)| e.count.to_string().len()).max().unwrap_or(5);
                    eprintln!(
                        "  {code:<code_w$} | {level:<level_w$} | {desc:<desc_w$} | {count:<count_w$}",
                        code = "CODE",
                        level = "LEVEL",
                        desc = "DESCRIPTION",
                        count = "COUNT",
                        code_w = code_w,
                        level_w = level_w,
                        desc_w = desc_w.max(11),
                        count_w = count_w.max(5)
                    );
                    eprintln!(
                        "  {dash:-<code_w$}-+-{dash2:-<level_w$}-+-{dash3:-<desc_w$}-+-{dash4:-<count_w$}",
                        dash = "",
                        dash2 = "",
                        dash3 = "",
                        dash4 = "",
                        code_w = code_w,
                        level_w = level_w,
                        desc_w = desc_w.max(11),
                        count_w = count_w.max(5)
                    );
                    for (code, entry) in items {
                        eprintln!(
                            "  {code:<code_w$} | {level:<level_w$} | {desc:<desc_w$} | {count:<count_w$}",
                            code = code,
                            level = entry.level,
                            desc = entry.message,
                            count = entry.count,
                            code_w = code_w,
                            level_w = level_w,
                            desc_w = desc_w.max(11),
                            count_w = count_w.max(5)
                        );
                    }
                }
            } else {
                eprintln!("analysis_capture: rustc failed; errors at {}", errors_json.display());
            }
            if !parse_ok {
                eprintln!("analysis_capture: failed to parse errors jsonl");
            }
            let nodes_csv = output_dir.join("nodes.csv");
            if nodes_csv.exists() {
                if let Err(err) = upg_analysis::augment_with_errors(&output_dir, &errors_json) {
                    eprintln!("analysis_capture: failed to augment UPG with errors: {err}");
                }
            }
            std::process::exit(1);
        }
        if should_run_analysis_engine(crate_name.as_deref(), &crate_types, &output_dir) {
            if let Some(bin) = analysis_engine_bin(&output_dir) {
                let lock_path = output_dir.join(".analysis_engine.lock");
                if let Ok(meta) = fs::metadata(&lock_path) {
                    if let Ok(modified) = meta.modified() {
                        if let Ok(age) = SystemTime::now().duration_since(modified) {
                            if age > Duration::from_secs(60) {
                                let _ = fs::remove_file(&lock_path);
                            }
                        }
                    }
                }
                if let Ok(_lock) = OpenOptions::new().write(true).create_new(true).open(&lock_path) {
                    let status = Command::new(bin)
                        .args(["--dir", output_dir.to_string_lossy().as_ref(), "--phase", "all"])
                        .status();
                    let _ = fs::remove_file(&lock_path);
                    if let Ok(status) = status {
                        if !status.success() {
                            eprintln!("analysis_capture: analysis-engine failed with status {:?}", status.code());
                        }
                    } else {
                        eprintln!("analysis_capture: failed to execute analysis-engine");
                    }
                }
            }
        }
    } else {
        eprintln!("analysis_capture: rustc failed; errors at {}", errors_json.display());
        std::process::exit(1);
    }

    std::process::exit(0);
}

fn with_stderr_redirect<F, T>(path: &Path, f: F) -> std::io::Result<T>
where
    F: FnOnce() -> T,
{
    let file = File::create(path)?;
    let err_fd = file.as_raw_fd();
    unsafe {
        let saved = libc::dup(libc::STDERR_FILENO);
        if saved < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::dup2(err_fd, libc::STDERR_FILENO) < 0 {
            let err = std::io::Error::last_os_error();
            libc::close(saved);
            return Err(err);
        }
        let result = f();
        libc::dup2(saved, libc::STDERR_FILENO);
        libc::close(saved);
        Ok(result)
    }
}

fn parse_errors_jsonl(src: &Path, dst: &Path) -> std::io::Result<ErrorSummary> {
    let file = File::open(src)?;
    let reader = BufReader::new(file);
    let mut diagnostics: Vec<Value> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            let is_diag = value
                .get("$message_type")
                .and_then(|v| v.as_str())
                .map(|s| s == "diagnostic")
                .unwrap_or(false);
            if is_diag {
                diagnostics.push(value);
            }
        }
    }
    let mut out = File::create(dst)?;
    let payload = serde_json::json!({ "errors": diagnostics });
    out.write_all(serde_json::to_string_pretty(&payload)?.as_bytes())?;
    Ok(summarize_errors(&payload))
}

#[derive(Debug)]
struct ErrorSummary {
    count: usize,
    by_code: BTreeMap<String, ErrorCategory>,
}

#[derive(Debug)]
struct ErrorCategory {
    count: usize,
    message: String,
    level: String,
}

fn summarize_errors(payload: &Value) -> ErrorSummary {
    let mut by_code: BTreeMap<String, ErrorCategory> = BTreeMap::new();
    let errors = payload.get("errors").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for err in &errors {
        let code = err
            .get("code")
            .and_then(|c| c.get("code"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let message = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let level = err
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let entry = by_code.entry(code.to_string()).or_insert_with(|| ErrorCategory {
            count: 0,
            message: message.clone(),
            level: level.clone(),
        });
        entry.count += 1;
        if entry.message.is_empty() && !message.is_empty() {
            entry.message = message;
        }
        if entry.level != "error" && level == "error" {
            entry.level = "error".to_string();
        }
    }
    ErrorSummary {
        count: errors.len(),
        by_code,
    }
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn find_flag_values(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .collect()
}

fn project_root_from_out_dir(args: &[String]) -> Option<PathBuf> {
    let out_dir = args
        .windows(2)
        .find(|w| w[0] == "--out-dir")
        .map(|w| PathBuf::from(&w[1]))?;
    project_root_from_target_path(&out_dir)
}

fn project_root_from_env() -> Option<PathBuf> {
    std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from)
}

fn project_root_from_target_path(out_dir: &Path) -> Option<PathBuf> {
    let mut cursor = Some(out_dir);
    while let Some(path) = cursor {
        if path.file_name().and_then(|s| s.to_str()) == Some("target") {
            return path.parent().map(|p| p.to_path_buf());
        }
        cursor = path.parent();
    }
    None
}

fn should_run_analysis_engine(crate_name: Option<&str>, crate_types: &[String], output_dir: &Path) -> bool {
    if let Ok(primary) = std::env::var("CARGO_PRIMARY_PACKAGE") {
        if primary != "1" {
            return false;
        }
    } else {
        let pkg = std::env::var("CARGO_PKG_NAME").ok();
        if pkg.as_deref() != crate_name {
            return false;
        }
    }
    if !output_dir.join("nodes.csv").exists() {
        return false;
    }
    if crate_types.iter().any(|t| t == "bin") {
        return true;
    }
    crate_types.iter().any(|t| t == "lib" || t == "rlib")
}

fn analysis_engine_bin(output_dir: &Path) -> Option<PathBuf> {
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        let bin = target_dir.join("debug").join("analysis-engine");
        if bin.exists() {
            return Some(bin);
        }
    }
    let mut cursor = output_dir.parent();
    while let Some(dir) = cursor {
        let bin = dir.join("target").join("debug").join("analysis-engine");
        if bin.exists() {
            return Some(bin);
        }
        cursor = dir.parent();
    }
    None
}
