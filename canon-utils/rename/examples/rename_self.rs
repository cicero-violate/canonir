#![feature(rustc_private)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = rename::runner::run_rename_self_from_env()?;
    println!("status: {}", result.status);
    println!("report: {}", result.report_path.display());
    Ok(())
}
