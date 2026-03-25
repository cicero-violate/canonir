# Schema Upgrade 2: Enforced Payload + Causality

## Goal

Eliminate empty/null payload slots (input, output, delta) and enforce
causal parent linking at the type and macro level. Every event must
carry provenance — who called it, what it consumed, what it produced.

Selected constraints from `schema_upgrade_2.md`:

|  # | Constraint                             | Enforcement layer                       |
|----+----------------------------------------+-----------------------------------------|
|  1 | I,O ≠ ∅ — input/output/delta non-null  | proc macro field attributes + wire type |
|  5 | parent_ids non-empty (≥ 1 unless root) | type-level newtype + emit macro         |
|  7 | Payload shape lock (trait)             | proc macro auto-impl                    |
|  8 | No Default construction                | remove Default from all event derives   |
|  6 | Kind as enum (no free strings)         | proc macro on RuntimeEvent              |
| 12 | Serialization totality                 | const assertion in proc macro           |
| 10 | Time monotonicity                      | tlog writer runtime check               |

Deferred (architectural, not macro-enforceable now):
- Constraint 2 (deterministic state transition) — needs broader design
- Constraint 9 (ProducesDelta trait) — delta field coverage comes first
- Constraint 13 (Transform trait) — higher-order, plan separately

---

## Constraint 1 — Non-null input / output / delta

### Problem

Current `CanonPayload` in `wire.rs` after upgrade-1 has:
```rust
pub input:  Option<serde_json::Value>,
pub output: Option<serde_json::Value>,
pub delta:  Option<serde_json::Value>,
```
These are always `None` in practice. The math says `I, O ≠ ∅`.

### Solution A — Wire type change

Replace `Option<Value>` with non-optional `Value` in `CanonPayload`:

```rust
pub struct CanonPayload {
    pub input:  serde_json::Value,   // mandatory, never null
    pub output: serde_json::Value,   // mandatory, never null
    pub delta:  serde_json::Value,   // mandatory, never null
    pub meta:   CanonPayloadMeta,    // file + line, always present
}
```

`serde_json::Value::Null` is still a valid JSON value — the contract is that
callers must explicitly pass `json!({})` (empty object) rather than `None`.
An empty object is a valid empty input. `null` in JSON is forbidden; the
tlog writer validates this (Step 7 in this plan).

### Solution B — Field routing via proc macro attributes

`canon_event_struct!` gains three new per-field attributes:
- `#[input]`  — this field(s) form the input slot
- `#[output]` — this field(s) form the output slot
- `#[delta]`  — this field(s) form the delta slot

**Rules enforced at compile time by the proc macro:**

1. At least one field must be marked `#[input]` OR the struct must carry
   `#[no_input]` at the struct level (only allowed for pure events with no
   consumable input — e.g. `SessionStart`).
2. At least one field must be marked `#[output]`. **No exceptions. No escape
   hatch.** Every event produces observable output — at minimum a `success: bool`
   or `accepted: bool` that records whether the action it represents was
   received and acted upon. `#[no_output]` does not exist in this system.
3. A struct with no fields at all is a compile error.
4. `Option<T>` fields are forbidden unless the field carries `#[serde(default)]`
   AND the containing struct carries `#[allow_optional]` — explicit opt-in only.

**Emit macro routing:**

When `canon_emit!` constructs a `CanonPayload` it calls `CanonPayloadShape`
methods (see Constraint 7) to extract input/output/delta values from the
event struct fields rather than wrapping the whole struct in `data`.

**Proc macro compile error examples:**

```rust
// error: canon_event_struct! requires at least one #[output] field
// hint: add `#[output] success: bool` at minimum — every event must
//       record whether it was received and acted upon
canon_event_struct!(BadEvent { #[input] request_id: String });

// error: canon_event_struct! requires at least one #[input] field
// hint: add #[input] to a field, or add #[no_input] to the struct
canon_event_struct!(AlsoBad { #[output] success: bool });
```

---

## Constraint 5 — Parent linking required

### Problem

`CanonEvent.parent_ids: Vec<String>` is always `vec![]` in practice.
Every non-root event must name at least one cause.

### Solution A — Newtype `EventId`

Replace bare `String` with a newtype that cannot be constructed accidentally:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(String);

impl EventId {
    /// Only callable from the emit machinery — not pub.
    pub(crate) fn new(v: impl Into<String>) -> Self { Self(v.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

No `impl Default for EventId`. No `From<String>` impl. Construction is
controlled by the emit layer.

### Solution B — `parent_ids` enforcement in the wire type

```rust
pub struct CanonEvent {
    pub id:         EventId,
    pub parent_ids: Vec<EventId>,   // validated on construction
    pub actor:      String,
    pub kind:       String,
    pub ts:         u64,
    pub payload:    CanonPayload,
}

impl CanonEvent {
    /// Only constructor — enforces parent requirement.
    pub fn new(
        id: EventId,
        parent_ids: Vec<EventId>,
        actor: impl Into<String>,
        kind: impl Into<String>,
        ts: u64,
        payload: CanonPayload,
        root: bool,
    ) -> Self {
        assert!(root || !parent_ids.is_empty(),
            "CanonEvent requires at least one parent_id unless root=true");
        Self { id, parent_ids, actor: actor.into(), kind: kind.into(), ts, payload }
    }
}
```

No public struct literal construction — `new()` is the only path.

### Solution C — `canon_emit!` macro enforcement

The `canon_emit!` direct form gains a mandatory `parents` argument:

```rust
// root event (first event in a session):
canon_emit!(root; "actor", "kind", payload, &path)

// non-root (most events):
canon_emit!("actor", "kind", payload, &path, parents: &[parent_id_1, parent_id_2])
```

The proc macro statically distinguishes `root` vs `parents:` form.

- `root` form → calls `CanonEvent::new(..., root: true)` with empty `parent_ids`
- `parents:` form → calls `CanonEvent::new(..., root: false)` and passes the slice
- Any other form (no `parents:`, no `root`) → **compile error**:
  ```
  error: canon_emit! requires either `root;` prefix or `parents: &[...]` argument
  hint: every non-root event must declare its causal parents
  ```

For the emitter-routed form (`canon_emit!(emitter; Variant(payload))`), the
emitter itself carries a `ParentContext` that threads parent IDs through the
call chain — see Appendix A.

### Solution D — `EventEmitter` trait extension

```rust
pub trait EventEmitter: Send + Sync {
    fn emit_located(&self, event: RuntimeEvent, file: &'static str, line: u32);
    fn emit_with_parents(
        &self,
        event: RuntimeEvent,
        parent_ids: Vec<EventId>,
        file: &'static str,
        line: u32,
    );
    fn emit(&self, event: RuntimeEvent) {
        self.emit_located(event, "", 0);
    }
}
```

The runtime (EventRuntime) assigns a fresh `EventId` and attaches the
`parent_ids` when writing to the tlog.

---

## Constraint 7 — Payload shape lock

### Problem

No compile-time guarantee that an event type is a valid payload shape.
Any struct can be shoved into `serde_json::Value` via `to_value`.

### Solution — `CanonPayloadShape` trait

```rust
pub trait CanonPayloadShape: serde::Serialize + Sized {
    /// Serialize the fields marked #[input] into a Value.
    fn input_value(&self) -> serde_json::Value;
    /// Serialize the fields marked #[output] into a Value.
    fn output_value(&self) -> serde_json::Value;
    /// Serialize the fields marked #[delta] into a Value.
    fn delta_value(&self) -> serde_json::Value;
}
```

`canon_event_struct!` proc macro auto-implements `CanonPayloadShape` for
every generated struct by collecting fields tagged `#[input]`/`#[output]`/`#[delta]`
into a `serde_json::json!({...})` object per slot.

**Example — proc macro input:**

```rust
canon_event_struct!(LoopActed {
    #[input]  tick: u64,
    #[input]  action_kind: String,
    #[output] stdout: String,
    #[output] success: bool,
    #[delta]  exit_code: Option<i32>,
    duration_ms: u64,   // untagged → goes into payload.meta or dropped from slots
});
```

**Generated `CanonPayloadShape` impl:**

```rust
impl CanonPayloadShape for LoopActed {
    fn input_value(&self) -> serde_json::Value {
        serde_json::json!({ "tick": self.tick, "action_kind": self.action_kind })
    }
    fn output_value(&self) -> serde_json::Value {
        serde_json::json!({ "stdout": self.stdout, "success": self.success })
    }
    fn delta_value(&self) -> serde_json::Value {
        serde_json::json!({ "exit_code": self.exit_code })
    }
}
```

**`canon_emit!` using the trait:**

```rust
// Inside the macro expansion for direct-form emit:
let __payload = CanonPayload {
    input:  __event.input_value(),
    output: __event.output_value(),
    delta:  __event.delta_value(),
    meta:   CanonPayloadMeta { file: file!().to_string(), line: line!() },
};
```

The `CanonPayloadShape` bound is enforced at the `canon_emit!` call site — if
the event type does not impl the trait, it is a compile error.

---

## Constraint 8 — No Default construction

### Problem

`canon_event_struct!` currently derives `Default`, letting code construct
an event with all-zero/empty fields and emit it — a silent data loss path.

### Solution — Remove Default from derives

`canon_event_struct!` proc macro no longer emits `#[derive(Default)]`.

```rust
// Old derives emitted:
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]

// New derives emitted:
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
```

**Downstream breakage — `sample_all()`:**

`canon_event_enum!` currently generates `sample_all()` which calls
`<T>::default()`. This method must be removed or replaced:

```rust
// Remove sample_all() entirely from canon_event_enum! output.
// Callers that need test instances must construct them explicitly.
```

**Downstream breakage — serde deserialization defaults:**

Fields tagged `#[serde(default)]` still work for deserialization (this is
a `serde` attribute, not a `Default` bound requirement). These are unaffected.

**Fields with `Default` as a bound in generics:**

Any code using `T: Default` for event types must be updated to explicit
construction. Grep: `<$inner>::default()` in macro expansions, `Default::default()`
on event struct types.

---

## Constraint 6 — Kind as enum (no free strings)

### Problem

`CanonEvent.kind: String` at the wire level allows any string. Invalid
kinds are only caught at read/deserialize time.

### Solution — `EventKind` enum derived from `RuntimeEvent`

The `canon_event_enum!` proc macro, when applied to `RuntimeEvent`, emits
a parallel `EventKind` enum with the same variant names:

```rust
// Auto-generated alongside RuntimeEvent:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    Code, Debug, ErrorOccurred, Edit, Tick,
    LoopObserved, LoopPlanned, LoopActed, LoopVerified, LoopRewarded,
    GoodnessSnapshot, RouteTick, RouteSelected,
    Cargo, File, Bash, Llm, RequestDispatch, SubTaskResult, Analysis,
    RuntimeStateUpdated, NodeReady, NodeStarted, NodeCompleted, NodeFailed,
    CapabilityCompleted, CapabilityFailed, PolicyBaselineUpdated, GoalSelected,
    SystemConfigLoaded, AgentRegistered, PromptLoaded, ToolCall, ToolResult,
    ToolBatchSettled, GoalNodeCreated, GoalNodeRetracted, GoalNodeRewritten,
    GoalEdgeDefined, GoalGraphCheckpointed, CapabilityInvoked, CapabilityResolved,
}

impl EventKind {
    pub fn as_str(self) -> &'static str { ... }  // snake_case name
}
```

`CanonEvent.kind` changes type from `String` to `EventKind`:

```rust
pub struct CanonEvent {
    pub id:         EventId,
    pub parent_ids: Vec<EventId>,
    pub actor:      String,
    pub kind:       EventKind,      // ← enum, not String
    pub ts:         u64,
    pub payload:    CanonPayload,
}
```

For JSON serialization `EventKind` uses `#[serde(rename_all = "snake_case")]`
so wire format remains `"kind": "loop_acted"` — backwards compatible with
existing tlog readers that match on string.

The `canon_event_enum!` proc macro trigger: when the enum name is `RuntimeEvent`
(or is annotated `#[canon_kind_enum]`), emit the companion `EventKind` enum.

---

## Constraint 12 — Serialization totality

### Problem

`serde_json::to_value(payload).unwrap_or_default()` silently swallows
serialization failures.

### Solution — Compile-time Serialize bound + panic-on-failure

In `canon_event_struct!` proc macro, emit a `const _` static assertion:

```rust
// Emitted by proc macro for every generated struct:
const _: fn() = || {
    fn assert_serialize<T: serde::Serialize>() {}
    assert_serialize::<MyStruct>();
};
```

At runtime in `canon_emit!` Form 3 (direct tlog write), replace
`unwrap_or_default()` with an explicit `expect()`:

```rust
serde_json::to_value(&__event.input_value())
    .expect("canon_emit!: input field serialization must not fail")
```

Serialization failure is always a programming error (a non-serializable type
was tagged `#[input]`) — panic is correct here.

---

## Constraint 10 — Time monotonicity

### Problem

Nothing prevents a writer from emitting events with decreasing timestamps
(e.g. from clock adjustments, multithreaded writes).

### Solution — monotonic counter + assertion in `BinarySegmentWriter`

`BinarySegmentWriter` tracks `last_ts: AtomicU64`.

On each `write_canon_event`:
```rust
let prev = self.last_ts.load(Ordering::Relaxed);
if event.ts < prev {
    // Clock went backwards — clamp to prev + 1 rather than panic,
    // so writes always succeed but ts ordering is guaranteed.
    event.ts = prev + 1;
}
self.last_ts.store(event.ts, Ordering::Relaxed);
```

For the JSONL writer path same logic in `emit_canon_event_json`.

---

## Affected files

| File                                                  | Change                                                                                                                                         |
|-------------------------------------------------------+------------------------------------------------------------------------------------------------------------------------------------------------|
| `canon-utils/canon-runtime-events/src/wire.rs`        | `CanonPayload` fields non-optional; `CanonEvent.kind → EventKind`; `id/parent_ids` use `EventId` newtype; `CanonEvent::new()` constructor only |
| `canon-utils/canon-runtime-events/src/events.rs`      | Add `#[input]`/`#[output]`/`#[delta]` field tags to all existing event structs; remove `sample_all()` from enum expansions                     |
| `canon-utils/canon-runtime-events/src/emit.rs`        | Remove `unwrap_or_default()`; use `CanonEvent::new()`; route through `CanonPayloadShape` methods                                               |
| `canon-utils/canon-runtime-events/src/lib.rs`         | Export `EventId`, `EventKind`, `CanonPayloadShape`                                                                                             |
| `canon-utils/canon-proc-macros/src/lib.rs`            | Implement all changes to `canon_event_struct!`, `canon_event_enum!`, `canon_emit!`                                                             |
| `canon-utils/canon-runtime-events/src/tlog/binary.rs` | Add `last_ts: AtomicU64` to `BinarySegmentWriter`; enforce monotonic ts                                                                        |
| `canon-utils/canon-storage-eventlog/src/` (reader)    | Handle `EventKind` enum on deserialization; `parent_ids` as `Vec<EventId>`                                                                     |

---

## Execution order

### Phase 1 — Types (no behavior change)
1. Define `EventId` newtype in `wire.rs`
2. Define `EventKind` enum in `events.rs` (manual for now; proc macro emits it later)
3. Change `CanonPayload` fields from `Option<Value>` → `Value`
4. Change `CanonEvent.kind` from `String` → `EventKind`
5. Add `CanonEvent::new()` as the only constructor; make struct fields `pub(crate)`
6. Define `CanonPayloadShape` trait in `events.rs`

### Phase 2 — Proc macro updates
7. Update `canon_event_struct!` in `canon-proc-macros/src/lib.rs`:
   - Parse `#[input]`, `#[output]`, `#[delta]` field attributes
   - Emit compile error if no `#[input]` fields (unless `#[no_input]` on the struct)
   - Emit compile error if no `#[output]` fields — no escape hatch; every struct
     must name at least one output field; the error message must suggest adding
     `#[output] success: bool` as the minimum viable output
   - Remove `Default` from derives
   - Emit `CanonPayloadShape` impl
   - Emit `const _` serialization totality assertion
8. Update `canon_event_enum!`:
   - Remove `sample_all()` generation
   - When enum name is `RuntimeEvent` or carries `#[canon_kind_enum]`, emit `EventKind` companion enum
9. Update `canon_emit!`:
   - Add `root;` and `parents: &[...]` forms; reject form with neither
   - Route through `CanonPayloadShape` to populate `CanonPayload` slots
   - Use `CanonEvent::new()` constructor

### Phase 3 — Event struct annotation
10. Go through every `canon_event_struct!` call in `events.rs` and add `#[input]` / `#[output]` / `#[delta]` tags to fields. Guidelines:
    - **Input** = what the event consumed or received (requests, prompts, tick numbers, ids passed in)
    - **Output** = what the event produced (stdout, responses, results, success flags)
    - **Delta** = the difference / change introduced (reward delta, error count change, new nodes, diffs)
    - Untagged fields (timing, metadata, correlation ids) stay in the struct but don't appear in payload slots
11. Every event struct that currently has no natural output field must have one added.
    The minimum is a boolean that records whether the event was received and acted upon.
    See Appendix B for the required additions. There is no `#[no_output]`.

### Phase 4 — Emit call sites
12. Update all `canon_emit!` direct-form call sites to supply `parents: &[...]` or `root;`
13. Update `EventEmitter` trait with `emit_with_parents`
14. EventRuntime threads parent IDs through to the tlog write

### Phase 5 — Runtime enforcement
15. Add `last_ts: AtomicU64` to `BinarySegmentWriter`; implement monotonic clamp
16. Add tlog writer validation: reject events where `input == Value::Null` or `output == Value::Null`

### Phase 6 — Reader update
17. Update `parse_any_event` to deserialize `kind` as `EventKind` (with fallback string→enum for legacy logs)
18. Update `parent_ids` deserialization to produce `Vec<EventId>`

---

## Appendix A — ParentContext for emitter-routed events

The emitter-routed `canon_emit!(emitter; Variant(payload))` forms do not
write directly to the tlog; they route through `EventRuntime`. The runtime
must attach `parent_ids` when it serializes events.

Proposed approach: `EventEmitterHandle` wraps an `Arc<dyn EventEmitter>` and
carries a `ParentContext`:

```rust
pub struct EmitContext {
    pub parent_ids: Vec<EventId>,
}

pub trait EventEmitter: Send + Sync {
    fn emit_located(&self, event: RuntimeEvent, ctx: EmitContext, file: &'static str, line: u32);
}
```

`canon_emit!(emitter; Variant(payload), parents: &[p1, p2])` expands to:

```rust
emitter.emit_located(
    RuntimeEvent::Variant(payload),
    EmitContext { parent_ids: vec![p1.clone(), p2.clone()] },
    file!(), line!()
)
```

Thread-local `ParentContext` is an alternative if passing context through
every call site is too invasive — set before a block of emits, cleared after.

---

## Appendix B — Field tagging guidelines for existing events

High-value events and their recommended slot assignments:

Events that already have natural output fields — tag as-is:

| Event                 | input fields                                   | output fields                    | delta fields                 |
|-----------------------+------------------------------------------------+----------------------------------+------------------------------|
| `LoopObserved`        | `tick`, `goal_text`                            | `error_count`, `warning_count`   | `compiler_errors`            |
| `LoopPlanned`         | `tick`, `action_kind`, `action_payload`        | `reason`                         | `signals`                    |
| `LoopActed`           | `tick`, `action_kind`, `capability_request_id` | `stdout`, `success`, `exit_code` | `stderr`                     |
| `LoopVerified`        | `tick`                                         | `passed`, `compiler_clean`       | `error_count`, `diagnostics` |
| `LoopRewarded`        | `tick`, `errors_before`                        | `reward`, `halt`                 | `errors_after`, `delta_g`    |
| `ToolResult`          | `tool_call_id`, `kind`                         | `output`, `success`              | —                            |
| `RouteSelected`       | `tick`, `suggested_route`, `prompt`            | `approved_route`, `rationale`    | `gate_changed`               |
| `SubTaskResult`       | `dispatch_id`, `agent_id`                      | `success`, `output`              | `actions_taken`              |
| `CapabilityCompleted` | `request_id`, `capability`                     | `result`                         | —                            |

Events that require a new output field added to the struct definition:

| Event             | new field to add             | rationale                                          | input fields                      | delta fields |
|-------------------+------------------------------+----------------------------------------------------+-----------------------------------+--------------|
| `ToolCall`        | `accepted: bool`             | was the call accepted into the dispatch queue      | `tool_call_id`, `kind`, `payload` | —            |
| `GoalNodeCreated` | `created: bool`              | was the node successfully registered in the graph  | `node_id`, `description`, `deps`  | `caps`       |
| `LlmCall`         | `dispatched: bool`           | was the request handed off to the LLM layer        | `prompt`, `role`                  | —            |
| `RequestDispatch` | `dispatched: bool`           | was the sub-task successfully dispatched           | `parent_request_id`, `task_prompt`| `deps`       |
| `ErrorOccurred`   | `captured: bool`             | was the error captured and recorded                | `kind`, `source`, `message`       | `context`    |
| `Tick`            | `emitted: bool`              | did the tick propagate through the runtime         | `tick`                            | —            |
| `RouteTick`       | `emitted: bool`              | did the route tick register                        | `tick`                            | —            |

Codex must add these fields to the struct definitions in `events.rs` before
applying the `#[output]` tag. The field value at emit sites should be `true`
unless the emit site has explicit failure information, in which case `false`.
