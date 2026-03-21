# Implementation Plan: canon-check

## Score Target

Q = (S + D + L + F) / 4 — targeting Q = 1.0

| Variable         | Problem in previous plan                                | Fix                                                                           |
|------------------+---------------------------------------------------------+-------------------------------------------------------------------------------|
| S (separation)   | Check trait took `&CanonEvent` — coupled to event crate | Check trait takes `&serde_json::Value` — zero coupling                        |
| D (determinism)  | `run_checks` was pure — keep                            | unchanged                                                                     |
| L (loop safety)  | No self-filter                                          | Self-filter on `value["source"] == "canon_check"` in consumer                 |
| F (future-proof) | Checked `meta.file`, `meta.crate_name` by name          | Envelope check only: `meta` key present + `data` key present. No field names. |

Additionally: `Warn` vs `Err` had no enforced semantics. Defined below.

---

## Architecture

```
canon-check (pure library)
  deps: serde_json only — zero coupling to event system
  Check trait operates on &serde_json::Value
  CheckResult with defined semantics
  EnvelopeCheck (structural, future-proof)
  run_checks() / default_checks()

canon-runtime/consumers/check_consumer.rs
  serializes &CanonEvent → serde_json::Value
  self-filter: value["source"] == "canon_check" → return
  calls run_checks()
  emits failures via canon_emit_meta!
```

---

## CheckResult Semantics (defined)

```rust
pub enum CheckResult {
    Ok,
    Warn(Vec<CheckWarning>),   // degraded but recoverable — emit, continue
    Err(Vec<CheckError>),      // invariant violation — emit, future escalation point
}
```

- `Warn`: structural expectation not met. System continues. Flags drift.
- `Err`: hard invariant broken. Reserved for future enforcement (halt, alert).
- The consumer emits both. The distinction exists for future escalation without changing the check layer.

---

## Check Split (structural vs semantic)

| Kind       | Example                            | Future-proof            | Ships now                    |
|------------+------------------------------------+-------------------------+------------------------------|
| Structural | envelope present (`meta` + `data`) | yes                     | yes                          |
| Semantic   | `meta.file` is non-empty string    | no — field names change | no — not in default_checks() |

Semantic checks exist as a category but are NOT in `default_checks()`. They can be added by callers who opt in. This keeps the default suite stable.

---

## File 1 — `canon-utils/canon-check/Cargo.toml`

`canon-runtime-events` dep removed entirely. Only `serde_json`.

```toml
[package]
name = "canon-check"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
name = "canon_check"

[dependencies]
serde_json = { workspace = true }
```

---

## File 2 — `canon-utils/canon-check/src/lib.rs`

Replace entire file.

`Check` trait takes `&serde_json::Value`. No `CanonEvent`. No consumer. No emitter.

```rust
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

/// Warn = degraded, recoverable. Err = invariant violation, future escalation point.
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
// Check trait — operates on serialized event value, not typed CanonEvent.
// Decoupled from enum variants. Stable across schema evolution.
// ---------------------------------------------------------------------------

pub trait Check: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, event: &Value) -> CheckResult;
}

// ---------------------------------------------------------------------------
// EnvelopeCheck (structural)
//
// Checks that a DebugEvent payload has the envelope shape:
//   { "meta": <object>, "data": <any> }
//
// Does NOT inspect fields inside "meta" — future-proof against field renames.
// Only applies when top-level "kind" is present (is a canon event).
// Skips non-Debug events by checking absence of "payload" or non-object payload.
// ---------------------------------------------------------------------------

pub struct EnvelopeCheck;

impl Check for EnvelopeCheck {
    fn name(&self) -> &'static str {
        "envelope"
    }

    fn run(&self, event: &Value) -> CheckResult {
        // Only check events that have a payload object (DebugEvents written to tlog).
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

        CheckResult::Warn(vec![CheckWarning {
            check: self.name(),
            message: format!(
                "{}:{} missing envelope (has_meta={}, has_data={})",
                source, kind, has_meta, has_data
            ),
        }])
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run all checks against a serialized event. Returns only non-Ok results.
/// Caller owns emission, routing, and escalation.
pub fn run_checks(checks: &[Box<dyn Check>], event: &Value) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|c| c.run(event))
        .filter(|r| !r.is_ok())
        .collect()
}

/// Default suite: structural checks only. Stable across schema changes.
pub fn default_checks() -> Vec<Box<dyn Check>> {
    vec![Box::new(EnvelopeCheck)]
}
```

---

## File 3 — `canon-utils/canon-runtime/src/consumers/check_consumer.rs`

New file. Serializes `CanonEvent` before passing to checks. Self-filter on source field.

```rust
use canon_check::{default_checks, run_checks, Check, CheckResult};
use canon_event::{CanonEvent, EventConsumer, EventEmitterHandle, EventFilter};

pub struct CheckConsumer {
    checks: Vec<Box<dyn Check>>,
    emitter: Option<EventEmitterHandle>,
}

impl CheckConsumer {
    pub fn new() -> Self {
        Self { checks: default_checks(), emitter: None }
    }
}

impl Default for CheckConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventConsumer for CheckConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &CanonEvent) {
        // Serialize to Value — decouples checks from CanonEvent enum.
        let Ok(value) = serde_json::to_value(event) else {
            return;
        };

        // Self-filter: skip events emitted by this consumer.
        // canon_emit_meta! sets source = "canon_check".
        if value.get("source").and_then(|v| v.as_str()) == Some("canon_check") {
            return;
        }

        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };

        for result in run_checks(&self.checks, &value) {
            let (severity, items): (&str, Vec<(&str, &str)>) = match &result {
                CheckResult::Warn(w) => ("warn", w.iter().map(|x| (x.check, x.message.as_str())).collect()),
                CheckResult::Err(e) => ("error", e.iter().map(|x| (x.check, x.message.as_str())).collect()),
                CheckResult::Ok => continue,
            };
            for (check, message) in items {
                let payload = serde_json::json!({
                    "check": check,
                    "severity": severity,
                    "message": message,
                });
                let _ = canon_meta::canon_emit_meta!(emitter; "canon_check", severity, payload);
            }
        }
    }
}
```

---

## File 4 — `canon-utils/canon-runtime/src/consumers/mod.rs`

Add one line:

```rust
pub mod capability_executor;
pub mod check_consumer;       // add this
pub mod error_logger;
pub mod failure_store;
pub mod llm_executor;
```

---

## File 5 — `canon-utils/canon-runtime/src/bin/event_runtime.rs`

Line 9. One line change:

```rust
// Before:
use canon_check::CheckConsumer;

// After:
use canon_runtime::consumers::check_consumer::CheckConsumer;
```

`Box::new(CheckConsumer::new())` at line 710 is unchanged.

---

## What This Achieves

### canon-check has zero coupling to the event system

```
Before: canon-check → canon-runtime-events (CanonEvent enum)
After:  canon-check → serde_json only
```

Adding a new CanonEvent variant never touches canon-check.

### EnvelopeCheck survives field renames

```
Before: checks meta.file, meta.crate_name, meta.module, meta.line
After:  checks presence of "meta" key (object) + "data" key only
```

Renaming `crate_name` to `crate` in Meta struct → check still passes.

### Loop terminated at serialization boundary

```
on_event → serialize → value["source"] == "canon_check" → return
```

Emitted check events never reach run_checks. Guaranteed by source string, not by event type.

### CheckResult semantics enforced by comment contract

`Warn` = emit and continue. `Err` = future escalation point. Runtime today treats both as emit. Future runtime can branch on `Err` without changing canon-check.

---

## Summary

| File                                            | Action                                                                                         |
|-------------------------------------------------+------------------------------------------------------------------------------------------------|
| `canon-check/Cargo.toml`                        | Remove `canon-runtime-events` dep. `serde_json` only.                                          |
| `canon-check/src/lib.rs`                        | Replace. `Check` takes `&Value`. `EnvelopeCheck`. `run_checks`. `default_checks`. No consumer. |
| `canon-runtime/src/consumers/check_consumer.rs` | Create. Serializes event. Self-filter. Calls `run_checks`. Emits via `canon_emit_meta!`.       |
| `canon-runtime/src/consumers/mod.rs`            | Add `pub mod check_consumer;`                                                                  |
| `canon-runtime/src/bin/event_runtime.rs`        | Update import to `canon_runtime::consumers::check_consumer::CheckConsumer`                     |
