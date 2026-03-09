use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::invariant_errors::InvariantError;
use crate::invariants::{InvariantReport, INVARIANTS};

pub fn validate_report(report: &InvariantReport) -> Result<(), InvariantError> {
    for invariant in INVARIANTS {
        invariant(report)?;
    }
    Ok(())
}

pub fn validate_output_dir(output_dir: &Path) -> Result<(), InvariantError> {
    let path = output_dir.join("upg_invariants.json");
    let payload = fs::read_to_string(path).map_err(|_| InvariantError::must_have_valid_file_ids)?;
    let report: InvariantReport =
        serde_json::from_str(&payload).map_err(|_| InvariantError::must_have_valid_file_ids)?;
    validate_report(&report)
}
