# Implementation Plan: canon Event System Upgrade

## Goals

**Phase 1 — Metadata injection:** Wrap `canon_emit!` so every event can automatically include `file`, `crate_name`, `module`, and `line`. No breaking changes.

**Phase 2 — Single Path Event Architecture:** Collapse all emission paths to route exclusively through `canon_emit!`. Eliminate direct `.emit(CanonEvent::...)` call sites. Restrict low-level writer API. Result: one deterministic, auditable event path.

---

## Codebase Facts (Verified)

### Key existing symbols

| Symbol | Location | Notes |
|---|---|---|
| `canon_event_struct!` | `canon-macros/src/lib.rs:4` | Generates `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct` |
| `canon_emit!` | `canon-macros/src/lib.rs:51` | Two arms: emitter-routed and direct |
| `DebugEvent` | `canon-runtime-events/src/events.rs:111` | `{ source: String, kind: String, payload: serde_json::Value }` |
| `CanonEvent::Debug` | `canon-runtime-events/src/events.rs` | Wraps `DebugEvent` |
| `canon_event` | lib name of `canon-runtime-events` | Used as bare path in `canon_emit!` internals |

### `canon_emit!` arms (exact expansion)
```rust
// Emitter-routed:
$emitter.emit(canon_event::CanonEvent::Debug(canon_event::DebugEvent {
    source: $source.to_string(), kind: $kind.to_string(), payload: $payload
}))

// Direct:
let __event = canon_event::TlogEvent::new($source, $kind, $payload);
canon_event::write_event_auto($path, &__event)
```

### `canon-meta` current state
- `Cargo.toml`: `name = "canon-meta"`, `edition = "2024"`, no deps, no `[lib]`
- `src/main.rs`: `fn main() { println!("Hello, world!"); }` — stub binary only

### Workspace
- `canon-meta` is already a workspace member in root `Cargo.toml:31`
- `serde`, `serde_json` available as workspace deps

---

## Files to Create or Modify

### 1. `canon-utils/canon-meta/Cargo.toml` — MODIFY

Add a `[lib]` section and the required dependencies. Keep the existing `[[bin]]` or let the existing `main.rs` be the default binary (Cargo handles both automatically when both `src/lib.rs` and `src/main.rs` exist).

```toml
[package]
name = "canon-meta"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
name = "canon_meta"

[dependencies]
canon-macros = { path = "../canon-macros" }
canon-runtime-events = { path = "../canon-runtime-events" }
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
```

---

### 2. `canon-utils/canon-meta/src/lib.rs` — CREATE (new file)

This file provides:
- `Meta` struct (typed, serializable)
- `capture_meta!` macro (expands `file!`, `line!`, `module_path!`, `env!` at call site)
- `canon_emit_meta!` macro (wraps payload, delegates to `canon_emit!`)

```rust
use canon_macros::canon_event_struct;

canon_event_struct!(Meta {
    file: String,
    crate_name: String,
    module: String,
    line: u32,
});

/// Capture source location metadata at the call site.
/// Must be a macro so that file!/line!/module_path!/env! expand at the caller.
#[macro_export]
macro_rules! capture_meta {
    () => {
        canon_meta::Meta {
            file: file!().to_string(),
            crate_name: env!("CARGO_PKG_NAME").to_string(),
            module: module_path!().to_string(),
            line: line!(),
        }
    };
}

/// Emit a canonical event with automatic source metadata injected into the payload.
///
/// Wraps the existing `canon_emit!` — does NOT replace it.
///
/// Emitter-routed form:
/// ```rust,ignore
/// canon_emit_meta!(emitter; "source", "kind", payload);
/// ```
///
/// Direct form:
/// ```rust,ignore
/// canon_emit_meta!("source", "kind", payload, &tlog_path)?;
/// ```
///
/// Output payload shape:
/// ```json
/// { "meta": { "file": "...", "crate_name": "...", "module": "...", "line": 42 }, "data": <original> }
/// ```
///
/// Callers must have `canon_event` and `serde_json` as dependencies (same requirement as `canon_emit!`).
#[macro_export]
macro_rules! canon_emit_meta {
    // Emitter-routed form
    ($emitter:expr; $source:expr, $kind:expr, $payload:expr) => {{
        let __meta = canon_meta::capture_meta!();
        let __wrapped = serde_json::json!({
            "meta": __meta,
            "data": $payload,
        });
        canon_event::canon_emit!($emitter; $source, $kind, __wrapped)
    }};
    // Direct form
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __meta = canon_meta::capture_meta!();
        let __wrapped = serde_json::json!({
            "meta": __meta,
            "data": $payload,
        });
        canon_event::canon_emit!($source, $kind, __wrapped, $path)
    }};
}
```

---

## Files NOT Modified

| File | Reason |
|---|---|
| `canon-macros/src/lib.rs` | `canon_emit!` untouched per spec |
| `canon-runtime-events/src/events.rs` | `DebugEvent` struct untouched |
| `canon-runtime-events/src/lib.rs` | No changes needed |
| All consumer crates | Existing `canon_emit!` calls remain valid |

---

## Dependency Chain

```
canon_emit_meta! caller
  → canon_meta (provides macro)
    → canon-macros (for canon_event_struct!)
    → canon-runtime-events (for canon_event::canon_emit! path used in macro body)
    → serde_json (for json! used in macro body)
```

Callers of `canon_emit_meta!` need in their `Cargo.toml`:
- `canon-meta = { path = "../canon-meta" }` (or appropriate relative path)
- `canon-runtime-events` / `canon_event` (already required by `canon_emit!` users)
- `serde_json` (already present in most crates)

---

## Correctness Constraints

### Why `capture_meta!` must be a macro, not a function

`file!()`, `line!()`, and `module_path!()` are compiler built-in macros that expand to the location of their *call site*. If wrapped in a regular function, they would capture the function's own location inside `canon-meta`, not the caller's location. The `capture_meta!` macro expands inline at the call site, ensuring accurate metadata.

### `$crate::` hygiene in `capture_meta!`

Inside `capture_meta!`, `canon_meta::Meta { ... }` is used (not `$crate::Meta`) because `capture_meta!` is called *from within* `canon_emit_meta!`'s expansion, where `$crate` would refer to the caller's crate, not `canon_meta`. Using the full path `canon_meta::Meta` is unambiguous.

### Payload wrapping only at emission time

The `DebugEvent.payload` field is `serde_json::Value`. The wrapping `{ "meta": ..., "data": ... }` is a `serde_json::Value` produced by `json!()` — it satisfies the type without any struct changes.

---

## Migration Strategy

- All existing `canon_emit!(...)` call sites compile unchanged.
- New call sites use `canon_emit_meta!(...)` to gain automatic metadata.
- Gradual adoption: replace calls crate-by-crate as needed.
- No flag day required.

---

## Expected Output JSON Shape

```json
{
  "source": "my-crate",
  "kind": "some.event",
  "payload": {
    "meta": {
      "file": "src/consumers/observe.rs",
      "crate_name": "canon-observe",
      "module": "canon_observe",
      "line": 87
    },
    "data": {
      "...": "original payload fields"
    }
  }
}
```

---

## Phase 1 Summary of Changes

| File | Action | Lines Changed (approx) |
|---|---|---|
| `canon-utils/canon-meta/Cargo.toml` | Modify — add `[lib]`, add 4 deps | +8 |
| `canon-utils/canon-meta/src/lib.rs` | Create — Meta struct + 2 macros | ~50 |

Total: 2 files. No existing files broken.

---

# Phase 2 — Single Path Event Architecture

## Invariant

Every event emission must pass through `canon_emit!`. No direct `.emit(CanonEvent::...)` calls at non-macro call sites. No raw writer calls from consumer crates.

## Codebase Facts (Verified)

### Direct `.emit(CanonEvent::...)` call sites to eliminate

| File | Lines | Variants used |
|---|---|---|
| `canon-act/src/lib.rs` | 148, 184, 218, 228, 282, 292, 348, 359, 447, 471, 497, 676, 685, 841 | `LoopActed`, `Debug`, `ToolCall`, `CapabilityRequested`, `ToolResult` |
| `canon-plan/src/lib.rs` | 285, 292, 454, 460 | `ToolCall`, `CapabilityRequested`, `LoopPlanned`, `ToolResult` |
| `canon-reward/src/lib.rs` | 73, 95, 122, 165, 167 | `LoopRewarded`, `Debug`, `ErrorOccurred` |
| `canon-verify/src/lib.rs` | 121, 187 | `LoopVerified` |
| `canon-observe/src/lib.rs` | 60 | `LoopObserved` |
| `canon-llm-runtime/src/ws_server.rs` | 154 | `Debug` |
| `canon-runtime/src/lib.rs` | 703, 708 | `CapabilityRequested` |
| `canon-runtime/src/consumers/capability_executor.rs` | 58, 62 | (variable `event`) |
| `canon-runtime/src/consumers/llm_executor.rs` | 102, 114, 248, 283, 294 | `ErrorOccurred`, `CapabilityFailed`, `CapabilityCompleted` |

Total: ~33 call sites across 9 files.

### Raw writer calls to restrict (not in macro path)

| File | Symbol | Action |
|---|---|---|
| `canon-runtime/src/lib.rs:572` | `write_event_auto` | Internal runtime drain — keep, already isolated |
| `canon-runtime-events/src/emit.rs:50` | `emit_event` | Make `pub(crate)` |
| `canon-runtime-events/src/tlog/writer.rs:81` | `emit_event_json` | Make `pub(crate)` within tlog module |
| `canon-runtime-events/src/lib.rs:39` | re-export of `emit_event_json` | Remove from public API |

### Existing `canon_emit!` call sites (already canonical — keep as-is)

`canon-verify/src/lib.rs:92`, `canon-runtime/src/bootstrap.rs:139,177`, `canon-runtime/src/bin/*.rs`, `canon-runtime-events/src/bin/*.rs`, `canon-tools-editor/src/*.rs`, `canon-runtime/src/consumers/error_logger.rs:59`, `canon-runtime/src/consumers/llm_executor.rs:85,121,237,258,295,462`, `canon-builder/src/process.rs:29,52,66,92`, `canon-tools-analysis/src/capabilities/events.rs:6`

---

## Δ1 — Extend `canon_emit!` with Typed Variant Arm

**File:** `canon-utils/canon-macros/src/lib.rs`

Add a third arm to the existing `canon_emit!` macro that accepts a typed `CanonEvent` variant directly:

```rust
// New arm — typed variant form
($emitter:expr; $variant:ident($inner:expr)) => {{
    $emitter.emit(canon_event::CanonEvent::$variant($inner))
}};
```

Insert this arm **before** the existing emitter-routed arm (the `$source:expr, $kind:expr` arm) so the more specific pattern matches first.

**Full updated macro:**

```rust
#[macro_export]
macro_rules! canon_emit {
    // NEW: Typed variant form — canon_emit!(emitter; LoopPlanned(payload))
    ($emitter:expr; $variant:ident($inner:expr)) => {{
        $emitter.emit(canon_event::CanonEvent::$variant($inner))
    }};
    // Existing: Debug/DebugEvent form — canon_emit!(emitter; "source", "kind", payload)
    ($emitter:expr; $source:expr, $kind:expr, $payload:expr) => {{
        $emitter.emit(canon_event::CanonEvent::Debug(canon_event::DebugEvent {
            source: $source.to_string(),
            kind: $kind.to_string(),
            payload: $payload,
        }))
    }};
    // Existing: Direct form — canon_emit!("source", "kind", payload, &path)?
    ($source:expr, $kind:expr, $payload:expr, $path:expr) => {{
        let __event = canon_event::TlogEvent::new($source, $kind, $payload);
        canon_event::write_event_auto($path, &__event)
    }};
}
```

**Backward compatibility:** Both existing arms are unchanged. All current call sites compile as-is.

**Disambiguation note:** The `$variant:ident($inner:expr)` arm is distinguished from the `$source:expr, $kind:expr, $payload:expr` arm because the former has no comma before the `(` — Rust's macro parser disambiguates on the token tree structure. However, to be safe, use `ident` vs `expr` at the arm boundary: an `ident` followed by `(` cannot match `expr, expr, expr`. No conflict.

---

## Δ2 — Replace All Direct `.emit(CanonEvent::...)` Calls

**Pattern to replace:**
```rust
emitter.emit(CanonEvent::X(payload));
```

**Replacement:**
```rust
canon_emit!(emitter; X(payload));
```

**Special case — `DebugEvent` inline construction:**
```rust
// Before:
emitter.emit(CanonEvent::Debug(DebugEvent { source: s.to_string(), kind: k.to_string(), payload }));

// After (use existing Debug arm):
canon_emit!(emitter; "source-str", "kind-str", payload);
```

**Special case — `capability_executor.rs` variable event:**
```rust
// Before:
emitter.emit(event);  // event: CanonEvent

// Keep as-is — this is already a CanonEvent value, not a construction site.
// This call site is in the runtime drain/dispatch path and is structurally correct.
// Do not replace.
```

### File-by-file replacements

#### `canon-act/src/lib.rs`
- Lines 148, 447, 471, 685, 841: `emitter.emit(CanonEvent::LoopActed(...))` → `canon_emit!(emitter; LoopActed(...))`
- Line 184: `emitter.emit(CanonEvent::Debug(DebugEvent {...}))` → `canon_emit!(emitter; "act_consumer", "debug", payload)`
- Lines 218, 282, 348: `emitter.emit(CanonEvent::ToolCall(...))` → `canon_emit!(emitter; ToolCall(...))`
- Lines 228, 292, 359: `emitter.emit(CanonEvent::CapabilityRequested(...))` → `canon_emit!(emitter; CapabilityRequested(...))`
- Lines 497, 676: `emitter.emit(CanonEvent::ToolResult(...))` → `canon_emit!(emitter; ToolResult(...))`

#### `canon-plan/src/lib.rs`
- Lines 285, 282: `emitter.emit(CanonEvent::ToolCall(...))` → `canon_emit!(emitter; ToolCall(...))`
- Line 292: `emitter.emit(CanonEvent::CapabilityRequested(...))` → `canon_emit!(emitter; CapabilityRequested(...))`
- Line 454: `emitter.emit(CanonEvent::LoopPlanned(...))` → `canon_emit!(emitter; LoopPlanned(...))`
- Line 460: `emitter.emit(CanonEvent::ToolResult(...))` → `canon_emit!(emitter; ToolResult(...))`

#### `canon-reward/src/lib.rs`
- Lines 73, 122, 165: `emitter.emit(CanonEvent::LoopRewarded(...))` → `canon_emit!(emitter; LoopRewarded(...))`
- Line 95: `emitter.emit(CanonEvent::Debug(DebugEvent {...}))` → `canon_emit!(emitter; "reward_consumer", "debug", payload)`
- Line 167: `emitter.emit(CanonEvent::ErrorOccurred(...))` → `canon_emit!(emitter; ErrorOccurred(...))`

#### `canon-verify/src/lib.rs`
- Lines 121, 187: `emitter.emit(CanonEvent::LoopVerified(...))` → `canon_emit!(emitter; LoopVerified(...))`

#### `canon-observe/src/lib.rs`
- Line 60: `emitter.emit(CanonEvent::LoopObserved(...))` → `canon_emit!(emitter; LoopObserved(...))`

#### `canon-llm-runtime/src/ws_server.rs`
- Line 154: `e.emit(CanonEvent::Debug(DebugEvent {...}))` → `canon_emit!(e; "ws_server", "debug", payload)` or extract source/kind from local vars

#### `canon-runtime/src/lib.rs`
- Lines 703, 708: `emitter.emit(CanonEvent::CapabilityRequested(...))` → `canon_emit!(emitter; CapabilityRequested(...))`

#### `canon-runtime/src/consumers/llm_executor.rs`
- Lines 102, 283: `emitter.emit(CanonEvent::ErrorOccurred(...))` → `canon_emit!(emitter; ErrorOccurred(...))`
- Lines 114, 294: `emitter.emit(CanonEvent::CapabilityFailed(...))` → `canon_emit!(emitter; CapabilityFailed(...))`
- Line 248: `emitter.emit(CanonEvent::CapabilityCompleted(...))` → `canon_emit!(emitter; CapabilityCompleted(...))`

#### `canon-runtime/src/consumers/capability_executor.rs`
- Lines 58, 62: `emitter.emit(event)` — **leave unchanged** (dispatch path, `event` is already a `CanonEvent`)

---

## Δ3 — Restrict Writer Layer

Make low-level writer functions inaccessible from outside `canon-runtime-events`. The `canon_emit!` direct arm is the only sanctioned external path to `write_event_auto`.

**File: `canon-utils/canon-runtime-events/src/emit.rs`**

Change `emit_event` from `pub` to `pub(crate)`:
```rust
// Before:
pub fn emit_event(source: &str, kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {

// After:
pub(crate) fn emit_event(source: &str, kind: &str, payload: Value, tlog_path: &Path) -> Result<()> {
```

**File: `canon-utils/canon-runtime-events/src/tlog/writer.rs`**

Change `emit_event_json` from `pub` to `pub(crate)`:
```rust
// Before:
pub fn emit_event_json(path: &Path, ...) -> Result<()> {

// After:
pub(crate) fn emit_event_json(path: &Path, ...) -> Result<()> {
```

**File: `canon-utils/canon-runtime-events/src/tlog/mod.rs`**

Remove `emit_event_json` from the `pub use` line:
```rust
// Before:
pub use writer::{emit_event_json, TlogWriter};

// After:
pub use writer::TlogWriter;
```

**File: `canon-utils/canon-runtime-events/src/lib.rs`**

Remove `emit_event_json` from the public re-export:
```rust
// Before:
pub use tlog::{emit_event_json, is_binary_tlog, maybe_rotate, BinarySegmentWriter, BinaryTlogWriter, RotateConfig, SegmentConfig, TlogEvent, TlogWriter};

// After:
pub use tlog::{is_binary_tlog, maybe_rotate, BinarySegmentWriter, BinaryTlogWriter, RotateConfig, SegmentConfig, TlogEvent, TlogWriter};
```

**`write_event_auto` stays `pub`** — it is called from within the `canon_emit!` macro body (macro expansions run at the call site and need the symbol to be pub).

---

## Δ4 — Compile-Time Enforcement

Rust does not support custom lint passes without a compiler plugin. Use a CI grep gate instead:

**Enforcement script** (add to CI / `Makefile`):
```bash
# Fail if any direct .emit(CanonEvent:: call exists outside macro definitions
! rg --type rust '\.emit\(CanonEvent::' canon-utils \
    --glob '!canon-macros/**' \
    --glob '!canon-runtime-events/**'
```

This grep returns non-zero (CI pass) when zero matches are found. The `!` prefix inverts the exit code. Exclude `canon-macros` (definition site) and `canon-runtime-events` (internal runtime drain at `lib.rs:572` which is permitted).

The `capability_executor.rs` dispatch (`emitter.emit(event)`) matches `\.emit\(` but not `\.emit\(CanonEvent::` since `event` is a variable — this call site passes the grep gate automatically.

---

## Δ5 — Normalize Inline Event Construction

Where event structs are constructed inline at call sites, keep construction inline within `canon_emit!()`. Do not extract to separate `let` bindings unless the struct is referenced more than once.

**Preferred style:**
```rust
canon_emit!(emitter; LoopPlanned(LoopPlanned {
    action_kind: action.kind.clone(),
    payload: action.payload.clone(),
    reason: plan.reason.clone(),
    ..Default::default()
}));
```

**Not:**
```rust
let payload = CanonEvent::LoopPlanned(LoopPlanned { ... });
emitter.emit(payload);
```

---

## Migration Order

1. **Δ1** — Extend `canon_emit!` macro (unblocks all other steps; zero risk, additive only)
2. **Δ2** — Replace all direct `.emit(CanonEvent::...)` call sites (mechanical substitution)
3. **Δ3** — Restrict writer layer (`pub` → `pub(crate)`, remove re-exports)
4. **Δ4** — Add CI grep gate
5. **Verify** — `rg '\.emit\(CanonEvent::' canon-utils --glob '!canon-macros/**' --glob '!canon-runtime-events/**'` returns zero matches

Steps 1 and 2 can be done in one pass per file. Step 3 must come after step 2 (restricting the API before migrating callers would break compilation). Step 4 is last (enforcement after full migration).

---

## Phase 2 Summary of Changes

| File | Action |
|---|---|
| `canon-macros/src/lib.rs` | Add typed variant arm to `canon_emit!` |
| `canon-act/src/lib.rs` | Replace ~14 direct emit calls |
| `canon-plan/src/lib.rs` | Replace ~4 direct emit calls |
| `canon-reward/src/lib.rs` | Replace ~5 direct emit calls |
| `canon-verify/src/lib.rs` | Replace ~2 direct emit calls |
| `canon-observe/src/lib.rs` | Replace ~1 direct emit call |
| `canon-llm-runtime/src/ws_server.rs` | Replace ~1 direct emit call |
| `canon-runtime/src/lib.rs` | Replace ~2 direct emit calls |
| `canon-runtime/src/consumers/llm_executor.rs` | Replace ~5 direct emit calls |
| `canon-runtime-events/src/emit.rs` | `pub` → `pub(crate)` on `emit_event` |
| `canon-runtime-events/src/tlog/writer.rs` | `pub` → `pub(crate)` on `emit_event_json` |
| `canon-runtime-events/src/tlog/mod.rs` | Remove `emit_event_json` from `pub use` |
| `canon-runtime-events/src/lib.rs` | Remove `emit_event_json` from public re-export |

Total: 13 files modified. No new files. All changes are mechanical substitutions or visibility restrictions.
