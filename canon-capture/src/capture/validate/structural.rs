use anyhow::{bail, Result};
use canon::ir::CanonIR;

pub fn validate(canon: &CanonIR) -> Result<()> {
    validate_no_alloc_artifacts(canon)?;
    validate_no_malformed_paths(canon)?;
    validate_global_csr(canon)?;
    Ok(())
}

fn validate_no_alloc_artifacts(canon: &CanonIR) -> Result<()> {
    if canon.name_intern.vec.iter().any(|s| s.contains("{alloc") || s.starts_with("alloc") || s.contains("promoted[")) {
        bail!("Invariant violation: MIR alloc/debug artifact leaked into Canon name interner");
    }
    Ok(())
}

fn validate_no_malformed_paths(canon: &CanonIR) -> Result<()> {
    for p in &canon.path_intern.vec {
        if p.split("::").any(|seg| seg.is_empty() || seg == "_" || seg.starts_with('_')) {
            bail!("Invariant violation: malformed/private helper path segment in Canon path interner");
        }
    }
    Ok(())
}

fn validate_global_csr(canon: &CanonIR) -> Result<()> {
    if let Err(msg) = canon.validate_global_csr() {
        bail!("Invariant violation: {msg}");
    }
    Ok(())
}
