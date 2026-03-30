use serde_json::Value;

// ---------------------------------------------------------------------------
// Result types — defined semantics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CheckWarning {
    pub check: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CheckError {
    pub check: &'static str,
    pub message: String,
}

/// Warn = degraded/recoverable. Err = invariant violation (reserved for future escalation).
#[derive(Debug, Clone)]
pub enum CheckResult {
    Ok,
    Warn(Vec<CheckWarning>),
    Err(Vec<CheckError>),
}

impl CheckResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, CheckResult::Ok)
    }
}

// ---------------------------------------------------------------------------
// Check trait — operates on serialized events, not typed enums.
// ---------------------------------------------------------------------------

pub trait Check: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, event: &Value) -> CheckResult;
}

// ---------------------------------------------------------------------------
// EnvelopeCheck (structural, future-proof)
// Ensures payload has both "meta" (object) and "data" keys.
// ---------------------------------------------------------------------------

pub struct EnvelopeCheck;

impl Check for EnvelopeCheck {
    fn name(&self) -> &'static str {
        "envelope"
    }

    fn run(&self, event: &Value) -> CheckResult {
        let Some(payload) = event.get("payload").and_then(|v| v.as_object()) else {
            return CheckResult::Ok;
        };

        let has_meta = payload.get("meta").map_or(false, |v| v.is_object());
        let has_data = payload.contains_key("data");

        if has_meta && has_data {
            return CheckResult::Ok;
        }

        let source = event.get("source").and_then(|v| v.as_str()).unwrap_or("unknown");
        let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");

        CheckResult::Warn(vec![CheckWarning { check: self.name(), message: format!("{}:{} missing envelope (has_meta={}, has_data={})", source, kind, has_meta, has_data) }])
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run all checks against a serialized event, returning only non-Ok results.
pub fn run_checks(checks: &[Box<dyn Check>], event: &Value) -> Vec<CheckResult> {
    checks.iter().map(|c| c.run(event)).filter(|r| !r.is_ok()).collect()
}

/// Default structural-only suite.
pub fn default_checks() -> Vec<Box<dyn Check>> {
    vec![Box::new(EnvelopeCheck), Box::new(SourceCheck), Box::new(KindCheck)]
}

// ---------------------------------------------------------------------------
// SourceCheck — require source field to exist and be non-empty.
// ---------------------------------------------------------------------------

pub struct SourceCheck;

impl Check for SourceCheck {
    fn name(&self) -> &'static str {
        "source"
    }

    fn run(&self, event: &Value) -> CheckResult {
        let source = event.get("source").and_then(|v| v.as_str());
        match source {
            Some(s) if !s.is_empty() => CheckResult::Ok,
            Some(_) => CheckResult::Warn(vec![CheckWarning { check: self.name(), message: "source field is empty".into() }]),
            None => CheckResult::Warn(vec![CheckWarning { check: self.name(), message: "source field missing".into() }]),
        }
    }
}

// ---------------------------------------------------------------------------
// KindCheck — require kind field to exist and be non-empty.
// ---------------------------------------------------------------------------

pub struct KindCheck;

impl Check for KindCheck {
    fn name(&self) -> &'static str {
        "kind"
    }

    fn run(&self, event: &Value) -> CheckResult {
        let kind = event.get("kind").and_then(|v| v.as_str());
        match kind {
            Some(k) if !k.is_empty() => CheckResult::Ok,
            Some(_) => CheckResult::Warn(vec![CheckWarning { check: self.name(), message: "kind field is empty".into() }]),
            None => CheckResult::Warn(vec![CheckWarning { check: self.name(), message: "kind field missing".into() }]),
        }
    }
}

// ---------------------------------------------------------------------------
// MetaFieldsCheck — optional, not in default_checks; validates meta.* presence.
// ---------------------------------------------------------------------------

pub struct MetaFieldsCheck;

impl Check for MetaFieldsCheck {
    fn name(&self) -> &'static str {
        "meta_fields"
    }

    fn run(&self, event: &Value) -> CheckResult {
        let Some(payload) = event.get("payload").and_then(|v| v.as_object()) else {
            return CheckResult::Ok;
        };
        let Some(meta) = payload.get("meta").and_then(|v| v.as_object()) else {
            return CheckResult::Ok;
        };

        let mut warnings = Vec::new();

        if meta.get("file").and_then(|v| v.as_str()).map_or(true, |s| s.is_empty()) {
            warnings.push(CheckWarning { check: self.name(), message: "meta.file is missing or empty".into() });
        }
        if meta.get("crate_name").and_then(|v| v.as_str()).map_or(true, |s| s.is_empty()) {
            warnings.push(CheckWarning { check: self.name(), message: "meta.crate_name is missing or empty".into() });
        }
        if meta.get("module").and_then(|v| v.as_str()).map_or(true, |s| s.is_empty()) {
            warnings.push(CheckWarning { check: self.name(), message: "meta.module is missing or empty".into() });
        }
        if meta.get("line").and_then(|v| v.as_u64()).map_or(true, |n| n == 0) {
            warnings.push(CheckWarning { check: self.name(), message: "meta.line is missing or zero".into() });
        }

        if warnings.is_empty() {
            CheckResult::Ok
        } else {
            CheckResult::Warn(warnings)
        }
    }
}
