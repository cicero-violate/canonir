//! CanonRepairSignal — structured lowering repair signals derived
//! from Rust compiler diagnostics.
//!
//! Converts BuildReport diagnostics into deterministic, typed
//! signals that the lowering engine can act upon.

use crate::{BuildReport, Diagnostic};

/// Canonical repair signals understood by the lowering loop.
#[derive(Debug, Clone)]
pub enum CanonRepairSignal {
    TypeMismatch {
        file: String,
        line: u32,
        expected: Option<String>,
        found: Option<String>,
    },
    MissingMethod {
        file: String,
        line: u32,
        method_name: Option<String>,
        type_name: Option<String>,
    },
    TraitBoundFailure {
        file: String,
        line: u32,
        trait_name: Option<String>,
        type_name: Option<String>,
    },
    UnitReturnDrift { file: String, line: u32 },
    NotIterator { file: String, line: u32 },
    Unknown { file: Option<String>, line: Option<u32> },
}

// ----------------------------
// Lightweight rendered parsers
// ----------------------------

fn parse_type_mismatch(rendered: Option<&str>)
    -> (Option<String>, Option<String>)
{
    if let Some(text) = rendered {
        for line in text.lines() {
            if line.contains("expected") && line.contains("found") {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    let expected = parts[0]
                        .replace("expected", "")
                        .replace("type", "")
                        .trim()
                        .to_string();
                    let found = parts[1]
                        .replace("found", "")
                        .trim()
                        .to_string();
                    return (Some(expected), Some(found));
                }
            }
        }
    }
    (None, None)
}

fn parse_trait_failure(rendered: Option<&str>)
    -> (Option<String>, Option<String>)
{
    if let Some(text) = rendered {
        for line in text.lines() {
            if line.contains("the trait")
                && line.contains("is not implemented for")
            {
                let parts: Vec<&str> = line.split('`').collect();
                if parts.len() >= 4 {
                    return (
                        Some(parts[1].to_string()),
                        Some(parts[3].to_string()),
                    );
                }
            }
        }
    }
    (None, None)
}

fn parse_missing_method(rendered: Option<&str>)
    -> (Option<String>, Option<String>)
{
    if let Some(text) = rendered {
        for line in text.lines() {
            if line.contains("no method named") {
                let parts: Vec<&str> = line.split('`').collect();
                if parts.len() >= 4 {
                    return (
                        Some(parts[1].to_string()),
                        Some(parts[3].to_string()),
                    );
                }
            }
        }
    }
    (None, None)
}

/// Classify a BuildReport into CanonRepairSignals.
pub fn classify(report: &BuildReport) -> Vec<CanonRepairSignal> {
    report
        .errors
        .iter()
        .map(classify_one)
        .collect()
}

fn classify_one(d: &Diagnostic) -> CanonRepairSignal {
    let (file, line) = match &d.primary_span {
        Some(span) => (span.file.clone(), span.line_start),
        None => ("<unknown>".to_string(), 0),
    };

    match d.error_code.as_str() {
        // mismatched types
        "E0308" => {
            let (expected, found) = parse_type_mismatch(d.rendered.as_deref());
            CanonRepairSignal::TypeMismatch {
                file,
                line,
                expected,
                found,
            }
        }

        // no method found
        "E0599" => {
            let (method_name, type_name) =
                parse_missing_method(d.rendered.as_deref());
            CanonRepairSignal::MissingMethod {
                file,
                line,
                method_name,
                type_name,
            }
        }

        // trait bound not satisfied
        "E0277" => {
            let (trait_name, type_name) =
                parse_trait_failure(d.rendered.as_deref());
            CanonRepairSignal::TraitBoundFailure {
                file,
                line,
                trait_name,
                type_name,
            }
        }

        _ => {
            // Heuristic: detect unit-return drift from rendered text
            if let Some(r) = &d.rendered {
                if r.contains("expected `String`, found `()`")
                    || r.contains("expected `&str`, found `()`")
                {
                    return CanonRepairSignal::UnitReturnDrift { file, line };
                }
                if r.contains("is not an iterator") {
                    return CanonRepairSignal::NotIterator { file, line };
                }
            }

            CanonRepairSignal::Unknown {
                file: Some(file),
                line: Some(line),
            }
        }
    }
}
