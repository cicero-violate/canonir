#![feature(rustc_private)]

extern crate rustc_driver;
use canon_rustc::runtime::RustcCaptureCallbacks;

fn exec_real_rustc(real_rustc: &str, args: &[String]) -> ! {
    let status = std::process::Command::new(real_rustc)
        .args(args)
        .status()
        .expect("failed to exec rustc");

    std::process::exit(status.code().unwrap_or(1));
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();

    if argv.len() < 2 {
        panic!("canon_kernel: missing rustc path");
    }

    let real_rustc = &argv[1];

    let is_probe = argv.iter().any(|a| a.starts_with("--print="))
        || argv.iter().any(|a| a == "-")
        || argv.iter().any(|a| a == "-vV" || a == "--version");

    if is_probe {
        exec_real_rustc(real_rustc, &argv[2..]);
    }

    let args: Vec<String> = std::iter::once(argv[0].clone())
        .chain(argv.iter().skip(2).cloned())
        .collect();

    let mut callbacks = RustcCaptureCallbacks::new(&argv);

    let result = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&args, &mut callbacks);
    });

    if result.is_err() {
        std::process::exit(1);
    }
}
