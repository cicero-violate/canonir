use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;

pub fn build_crate(crate_name: &str) -> Result<()> {
    let mut child = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg(crate_name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let name = crate_name.to_string();

    let out_handle = if let Some(out) = stdout {
        Some(thread::spawn({
            let name = name.clone();
            move || {
                let reader = BufReader::new(out);
                for line in reader.lines().flatten() {
                    println!("[build:{}] {}", name, line);
                }
            }
        }))
    } else {
        None
    };

    let err_handle = if let Some(err) = stderr {
        Some(thread::spawn({
            let name = name.clone();
            move || {
                let reader = BufReader::new(err);
                for line in reader.lines().flatten() {
                    println!("[build:{}] {}", name, line);
                }
            }
        }))
    } else {
        None
    };

    let status = child.wait()?;
    if let Some(handle) = out_handle {
        let _ = handle.join();
    }
    if let Some(handle) = err_handle {
        let _ = handle.join();
    }
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("build failed for {}", crate_name)
    }
}
