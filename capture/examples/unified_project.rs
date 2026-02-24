#![feature(rustc_private)]

use std::env;
use std::path::Path;

fn main() {
    let input = env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let path = Path::new(&input);

    println!("=== Unified Cargo Capture ===");

    match capture_rustc::run(path) {
        Ok(_) => {
            println!("Unified capture complete.");
        }
        Err(e) => {
            eprintln!("Capture failed: {e}");
            std::process::exit(1);
        }
    }
}
