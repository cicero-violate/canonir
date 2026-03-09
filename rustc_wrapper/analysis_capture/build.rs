use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Simple deterministic FNV-1a 64-bit hasher — no randomized seed.
struct FnvHasher(u64);
impl FnvHasher {
    fn new() -> Self { FnvHasher(0xcbf29ce484222325) }
    fn finish(&self) -> u64 { self.0 }
}
impl Hasher for FnvHasher {
    fn finish(&self) -> u64 { self.0 }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(&manifest_dir);
    let workspace_root = manifest_path
        .ancestors()
        .nth(2)
        .unwrap_or(&manifest_path)
        .to_path_buf();
    let upg_src = workspace_root.join("canon-utils").join("upg_analysis").join("src");
    let upg_manifest = workspace_root.join("canon-utils").join("upg_analysis").join("Cargo.toml");
    let analysis_src = manifest_path.join("src");
    let analysis_manifest = manifest_path.join("Cargo.toml");

    let mut hasher = FnvHasher::new();
    hash_dir(&analysis_src, &mut hasher);
    hash_file(&analysis_manifest, &mut hasher);
    hash_dir(&upg_src, &mut hasher);
    hash_file(&upg_manifest, &mut hasher);

    let hash = hasher.finish();
    println!("cargo:rustc-env=ANALYSIS_CAPTURE_SRC_HASH={hash}");

    emit_rerun_if_changed(&analysis_src);
    emit_rerun_if_changed(&upg_src);
    emit_rerun_if_changed(&analysis_manifest);
    emit_rerun_if_changed(&upg_manifest);
}

fn emit_rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                emit_rerun_if_changed(&entry.path());
            }
        }
    }
}

fn hash_dir(dir: &Path, hasher: &mut FnvHasher) {
    if let Ok(entries) = fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                hash_dir(&path, hasher);
            } else {
                hash_file(&path, hasher);
            }
        }
    }
}

fn hash_file(path: &Path, hasher: &mut FnvHasher) {
    if let Ok(data) = fs::read(path) {
        path.to_string_lossy().hash(hasher);
        data.hash(hasher);
    }
}
