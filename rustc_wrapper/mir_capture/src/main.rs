#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;

use mir_analysis::{extract_and_write, OutputConfig};
use rustc_driver::Callbacks;
use rustc_session::EarlyDiagCtxt;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

struct MirCaptureCallbacks {
    output_dir: PathBuf,
    include_fn_nodes: bool,
}

impl Callbacks for MirCaptureCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
        let config = OutputConfig {
            output_dir: self.output_dir.clone(),
            include_fn_nodes: self.include_fn_nodes,
        };
        if let Err(err) = extract_and_write(tcx, &config) {
            eprintln!("mir_capture: extraction failed: {err:?}");
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
    let is_probe = argv.iter().any(|a| a.starts_with("--print="))
        || argv.iter().any(|a| a == "-")
        || argv
            .windows(2)
            .any(|w| w[0] == "--crate-name" && w[1] == "___")
        || argv.iter().any(|a| a == "-vV" || a == "--version");

    if is_probe {
        exec_real_rustc(&real_rustc, &argv[2..], "probe");
    }

    let output_dir = crate_name
        .as_deref()
        .and_then(|name| project_root_from_metadata(name, &argv))
        .or_else(|| project_root_from_out_dir(&argv))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("mir");

    if let Err(err) = fs::create_dir_all(&output_dir) {
        eprintln!("mir_capture: failed to create output dir {output_dir:?}: {err}");
    }

    let args: Vec<String> = std::iter::once(argv[0].clone())
        .chain(argv.iter().skip(2).cloned())
        .collect();

    let include_fn_nodes = true;
    let mut callbacks = MirCaptureCallbacks {
        output_dir,
        include_fn_nodes,
    };

    let _diag = EarlyDiagCtxt::new(rustc_session::config::ErrorOutputType::default());
    let result = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&args, &mut callbacks);
    });

    if result.is_err() {
        std::process::exit(1);
    }

    std::process::exit(0);
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn project_root_from_out_dir(args: &[String]) -> Option<PathBuf> {
    let out_dir = args
        .windows(2)
        .find(|w| w[0] == "--out-dir")
        .map(|w| PathBuf::from(&w[1]))?;
    project_root_from_target_path(&out_dir)
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

fn project_root_from_metadata(crate_name: &str, args: &[String]) -> Option<PathBuf> {
    static MAP: OnceLock<BTreeMap<String, PathBuf>> = OnceLock::new();
    let map = MAP.get_or_init(|| build_package_map(args));
    map.get(crate_name).cloned()
}

fn build_package_map(args: &[String]) -> BTreeMap<String, PathBuf> {
    let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
    let manifest = project_root_from_out_dir(args)
        .and_then(find_nearest_manifest)
        .or_else(|| std::env::current_dir().ok().and_then(find_nearest_manifest));
    let Some(manifest_path) = manifest else {
        return out;
    };

    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest_path)
        .output();
    let Ok(output) = output else {
        return out;
    };
    if !output.status.success() {
        return out;
    }
    let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) else {
        return out;
    };
    let Some(packages) = value.get("packages").and_then(|v| v.as_array()) else {
        return out;
    };
    for pkg in packages {
        let Some(name) = pkg.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(manifest_path) = pkg.get("manifest_path").and_then(|v| v.as_str()) else {
            continue;
        };
        let path = Path::new(manifest_path)
            .parent()
            .map(|p| p.to_path_buf());
        if let Some(path) = path {
            out.insert(name.to_string(), path);
        }
    }
    out
}

fn find_nearest_manifest(start: PathBuf) -> Option<PathBuf> {
    let mut cursor = Some(start.as_path());
    while let Some(dir) = cursor {
        let candidate = dir.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}
