fn main() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    let commit = commit.trim();
    println!("cargo:rustc-env=CANON_COMMIT_ID={commit}");

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("cargo:rustc-env=CANON_BUILD_ID={commit}-{ts}");
    println!("cargo:rerun-if-changed=.git/HEAD");
}
