#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_session;

use std::process::Command;

fn exec_real_rustc(real_rustc: &str, args: &[String], reason: &str) -> ! {
    let status = Command::new(real_rustc)
        .args(args)
        .status()
        .unwrap_or_else(|err| panic!("failed to exec real rustc ({reason}): {err:?}"));
    std::process::exit(status.code().unwrap_or(1));
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();

    // wrapper <real_rustc> <rustc_args...>
    let real_rustc = argv.get(1).cloned().expect("missing real rustc path");
    let rustc_args: Vec<String> = argv.iter().skip(2).cloned().collect();

    // Skip probes / version / print queries
    let is_probe = rustc_args.iter().any(|a| a.starts_with("--print="))
        || rustc_args.iter().any(|a| a == "-")
        || rustc_args.iter().any(|a| a == "-vV" || a == "--version")
        || rustc_args
            .windows(2)
            .any(|w| w[0] == "--crate-name" && w[1] == "___");
    if is_probe {
        exec_real_rustc(&real_rustc, &rustc_args, "probe");
    }

    // Only capture primary crates (deps are delegated)
    let is_primary = std::env::var_os("CARGO_PRIMARY_PACKAGE").is_some();
    if !is_primary {
        exec_real_rustc(&real_rustc, &rustc_args, "dependency");
    }

    // 1) Run the real rustc FIRST so cargo's bookkeeping stays consistent.
    let status = Command::new(&real_rustc)
        .args(&rustc_args)
        .status()
        .expect("failed to exec real rustc");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    // 2) After a successful compile, run capture (second pass).
    //    This must NOT affect cargo's dependency graph.
    let capture_args: Vec<String> = std::iter::once(argv[0].clone())
        .chain(rustc_args.iter().cloned())
        .collect();

    capture_rustc::driver::run_hir(&capture_args);
    capture_rustc::driver::run_mir(&capture_args);

    std::process::exit(0);
}

