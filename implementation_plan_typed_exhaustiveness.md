# Implementation Plan: Typed Exhaustiveness + CFG Verifier + Watchdog

Three layers, applied in order. Each layer is independently useful; together they
make silent stalls impossible at compile time and impossible at runtime.

```
Layer 1 — EventOutcome type:  compile-time: every on_event path must declare what it did
Layer 2 — #[must_emit] macro:  compile-time: wildcard match arms on RuntimeEvent are a build error
Layer 3 — WatchdogConsumer:    runtime:      heartbeat thread + stage-staleness detector
```

Also fixes the three blocking diagnostics first (prerequisite).

---

## Prerequisite: Fix blocking diagnostics

### P1 — `.cargo/config.toml` lines 10 and 18: `-Fdead-code` → `-Ddead-code`

```toml
# BEFORE (line 10, [build] section):
"-Fdead-code",

# AFTER:
"-Ddead-code",

# BEFORE (line 18, [target.'cfg(all())'] section):
"-Fdead-code",

# AFTER:
"-Ddead-code",
```

### P2 — `analyst_consumer.rs` (from plan `implementation_plan_analyst_consumer.md`)

```rust
// BEFORE (CapabilityFailed arm):
eprintln!("[analyst_consumer] LLM capability failed: {:?}", fail.reason);

// AFTER (correct field name from events.rs:475):
eprintln!("[analyst_consumer] LLM capability failed: {}", fail.error);
```

### P3 — `None` pattern binding in `tlog.rs` and `agent.rs`

In both files, the for-loop gives `ev: &&Option<String>`. Match on the deref:

```rust
// BEFORE:
match ev {
    Some(s) => out.push_str(&format!("...\n{s}\n```\n")),
    None    => out.push_str(&format!("...: (none)\n")),
}

// AFTER (dereference so Rust sees Option<String> not &&Option<String>):
match ev.as_deref().as_ref().and_then(|o| o.as_deref()) {
    Some(s) => out.push_str(&format!("...\n{s}\n```\n")),
    None    => out.push_str(&format!("...: (none)\n")),
}
// OR more simply, just access through the double-ref:
match **ev {
    Some(ref s) => out.push_str(&format!("...\n{s}\n```\n")),
    None        => out.push_str(&format!("...: (none)\n")),
}
```

Apply the same fix to `agent.rs` wherever the same loop pattern appears.

---

## Layer 1: `EventOutcome` — typed return from every consumer

### Goal

`EventConsumer::on_event` currently returns `()`. Every silent `return;` or early
exit from a match arm discards the event with no trace. Changing the return type to
`EventOutcome` makes every exit path declare its intent, enforced by the compiler.

### 1a. Define `EventOutcome` in `canon-utils/canon-runtime-events/src/events.rs`

Add directly above the `EventConsumer` trait definition:

```rust
/// The outcome every consumer must return from `on_event`.
/// Returning unit `()` is a compile error; every path must declare intent.
#[derive(Debug)]
pub enum EventOutcome {
    /// Emit one event onto the bus.
    Emit(RuntimeEvent),
    /// Emit multiple events onto the bus (ordered).
    EmitMany(Vec<RuntimeEvent>),
    /// This consumer explicitly took no action this cycle.
    /// The &'static str is a required reason string — no silent no-ops.
    NoOp(&'static str),
    /// Emit an error event; the consumer's internal state is unchanged.
    Error(RuntimeEvent),
}
```

### 1b. Change `EventConsumer` trait signature

```rust
// BEFORE:
pub trait EventConsumer: Send + Sync {
    fn filter(&self) -> EventFilter;
    fn on_event(&mut self, event: &RuntimeEvent);
    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

// AFTER:
pub trait EventConsumer: Send + Sync {
    fn filter(&self) -> EventFilter;
    fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome;
    // set_emitter is kept for consumers that do async/deferred emission
    // (CapabilityExecutor). Pure-sync consumers should use EventOutcome instead.
    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}
```

Also export `EventOutcome` from `canon-utils/canon-runtime-events/src/lib.rs`:

```rust
// Add EventOutcome to the pub use in lib.rs:
pub use events::{
    ..., EventOutcome,
};
```

### 1c. Update `EventRuntime` to process returned outcomes

**File:** wherever `consumer.on_event(event)` is called in the runtime loop
(likely `canon-utils/canon-runtime/src/lib.rs` or `event_runtime.rs`).

```rust
// BEFORE:
consumer.on_event(event);

// AFTER:
match consumer.on_event(event) {
    EventOutcome::Emit(e)      => { runtime.emit_event(e).ok(); }
    EventOutcome::EmitMany(es) => { for e in es { runtime.emit_event(e).ok(); } }
    EventOutcome::NoOp(_)      => {}
    EventOutcome::Error(e)     => { runtime.emit_event(e).ok(); }
}
```

### 1d. Update all consumers — migration pattern

Apply this pattern to EVERY consumer. The same 4-step transform applies to each:

1. Change `fn on_event(&mut self, event: &RuntimeEvent)` → add `-> EventOutcome`
2. Replace `self.emitter.emit(X)` with `return EventOutcome::Emit(X)`
   (or accumulate into `EventOutcome::EmitMany`)
3. Replace `return;` / `_ => {}` with explicit `EventOutcome::NoOp("reason")`
4. Remove `self.emitter` field if the consumer only emits synchronously

**Consumers to migrate** (all in `canon-utils/canon-runtime/src/consumers/`):

| File | Current emission | Notes |
|---|---|---|
| `goal_gen_consumer.rs` | `emitter.emit(LlmCall)` | Pure sync — remove emitter field |
| `analyst_consumer.rs` | `emitter.emit(LlmCall)` | Pure sync — remove emitter field |
| `watchdog_consumer.rs` | (new — see Layer 3) | Pure sync from the start |
| `check_consumer.rs` | none | Returns `NoOp("check_consumer_passive")` everywhere |
| `error_logger.rs` | none | Returns `NoOp("error_logger_passive")` everywhere |
| `failure_store.rs` | none | Returns `NoOp("failure_store_passive")` everywhere |
| `agent_registry.rs` | none | Returns `NoOp("agent_registry_passive")` everywhere |
| `dispatch_consumer.rs` | `emitter.emit(...)` | May emit — migrate carefully |
| `goal_graph_consumer.rs` | `emitter.emit(...)` | May emit — migrate carefully |
| `capability_executor.rs` | spawns async tasks that call emitter | **Keep set_emitter**; return `NoOp("capability_executor_async")` |

For consumers in other crates that implement `EventConsumer`:

| Crate | Type | Notes |
|---|---|---|
| `canon-loop` | `LoopStageExecutor` | Returns `NoOp("loop_stage_async")` — emits via set_emitter |
| `canon-route` | `RouteExecutor` | Returns `NoOp("route_executor_async")` — emits via set_emitter |
| `canon-goodness` | `GoodnessConsumer` | Returns `EventOutcome::Emit(GoodnessSnapshot(...))` |

**Example migration — `goal_gen_consumer.rs`:**

```rust
// BEFORE (fields):
pub struct GoalGenConsumer {
    tlog_path: PathBuf,
    emitter: Option<EventEmitterHandle>,
    state: State,
}

// AFTER (remove emitter field):
pub struct GoalGenConsumer {
    tlog_path: PathBuf,
    state: State,
}

// BEFORE (on_event, Waiting arm):
(State::Waiting, RuntimeEvent::Tick(_)) => {
    let Some(emitter) = &self.emitter else { return; };
    let request_id = Uuid::new_v4().to_string();
    emitter.emit(RuntimeEvent::Llm(LlmCall { request_id: request_id.clone(), ... }));
    self.state = State::Pending(request_id);
}

// AFTER:
(State::Waiting, RuntimeEvent::Tick(_)) => {
    let request_id = Uuid::new_v4().to_string();
    self.state = State::Pending { request_id: request_id.clone(), ticks_waiting: 0 };
    return EventOutcome::Emit(RuntimeEvent::Llm(LlmCall {
        request_id,
        prompt: GOAL_GEN_PROMPT.to_string(),
        role: Some("goal_gen".to_string()),
        agent_id: Some("goal_gen_chatgpt".to_string()),
    }));
}

// BEFORE (catch-all):
_ => {}

// AFTER (explicit NoOp — no wildcard, see Layer 2):
(State::Done, _)
| (State::Pending { .. }, RuntimeEvent::Tick(_))
| (State::Waiting, _) => EventOutcome::NoOp("goal_gen_not_triggered"),
```

**Add tick timeout to `GoalGenConsumer::Pending`:**

```rust
// BEFORE:
Pending(String), // request_id

// AFTER:
Pending { request_id: String, ticks_waiting: u64 },
```

```rust
// In on_event, add Tick arm for Pending state:
(State::Pending { ticks_waiting, .. }, RuntimeEvent::Tick(_)) => {
    *ticks_waiting += 1;
    if *ticks_waiting >= 30 {
        eprintln!("[goal_gen_consumer] LLM timeout — retrying on next Tick");
        self.state = State::Waiting;
    }
    EventOutcome::NoOp("goal_gen_awaiting_llm")
}
```

---

## Layer 2: `#[must_emit]` — AST-level wildcard guard

### Goal

Prevent `_ => EventOutcome::NoOp(...)` wildcards in `match event { ... }` blocks
inside `on_event` implementations. Every arm must name a concrete `RuntimeEvent`
variant. This means adding a new variant to `RuntimeEvent` causes a compile error
in every consumer that hasn't acknowledged it — **CFG path completeness enforced
at compile time**.

### 2a. New proc-macro crate: `canon-utils/canon-proc-macros/`

**`canon-utils/canon-proc-macros/Cargo.toml`:**

```toml
[package]
name = "canon-proc-macros"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
proc-macro2 = { workspace = true }
quote = "1"
syn = { workspace = true, features = ["full", "visit"] }
```

Add `"canon-utils/canon-proc-macros"` to `[workspace]` members in root `Cargo.toml`.

Add `quote = "1"` to `[workspace.dependencies]` in root `Cargo.toml`.

### 2b. Implement the `#[must_emit]` attribute

**`canon-utils/canon-proc-macros/src/lib.rs`:**

```rust
use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemFn, visit::Visit, Expr, ExprMatch, Pat};

/// Applied to an `on_event` implementation.
///
/// Fails compilation if the function body contains any `match` expression
/// where one of the arms is a wildcard pattern (`_`) or binding pattern
/// at the top level of the arm list.
///
/// This ensures that when a new `RuntimeEvent` variant is added, every
/// `on_event` implementation that uses `#[must_emit]` must acknowledge it
/// explicitly rather than silently falling through.
///
/// # Usage
/// ```rust,ignore
/// #[must_emit]
/// fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
///     match event {
///         RuntimeEvent::Tick(_) => EventOutcome::Emit(...),
///         RuntimeEvent::LoopRewarded(_) => EventOutcome::NoOp("not relevant"),
///         // adding a new RuntimeEvent variant → compile error here until added
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn must_emit(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let mut checker = WildcardChecker { errors: Vec::new() };
    checker.visit_item_fn(&func);
    if !checker.errors.is_empty() {
        let msgs = checker.errors.join("\n");
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            format!(
                "#[must_emit]: wildcard or binding match arms are forbidden \
                 in event handlers — list every RuntimeEvent variant explicitly.\n{msgs}"
            ),
        )
        .to_compile_error()
        .into();
    }
    // Re-emit the function unchanged — the attribute is purely a guard.
    quote::quote! { #func }.into()
}

struct WildcardChecker {
    errors: Vec<String>,
}

impl<'ast> Visit<'ast> for WildcardChecker {
    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        // Only check matches that look like they match on a RuntimeEvent.
        // Heuristic: if ANY arm has a path pattern starting with "RuntimeEvent::"
        // then ALL arms must be non-wildcard.
        let has_event_arm = node.arms.iter().any(|arm| {
            pattern_is_runtime_event(&arm.pat)
        });
        if has_event_arm {
            for arm in &node.arms {
                if pattern_is_wildcard_or_binding(&arm.pat) {
                    self.errors.push(format!(
                        "  wildcard/binding arm found in RuntimeEvent match — \
                         add explicit arms for every variant instead"
                    ));
                }
            }
        }
        // Continue visiting nested matches.
        syn::visit::visit_expr_match(self, node);
    }
}

fn pattern_is_runtime_event(pat: &Pat) -> bool {
    match pat {
        Pat::TupleStruct(ts) => {
            let s = quote::quote!(#ts).to_string();
            s.contains("RuntimeEvent")
        }
        Pat::Or(or) => or.cases.iter().any(pattern_is_runtime_event),
        Pat::Tuple(t) => t.elems.iter().any(pattern_is_runtime_event),
        _ => false,
    }
}

fn pattern_is_wildcard_or_binding(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,   // `_`
        Pat::Ident(i) => {
            // A bare identifier without a path qualifier is a binding, not a variant.
            // Exception: `ref`, `mut` bindings inside structs are fine.
            i.subpat.is_none() && i.by_ref.is_none()
                && !i.ident.to_string().starts_with(|c: char| c.is_uppercase())
        }
        Pat::Or(or) => or.cases.iter().any(pattern_is_wildcard_or_binding),
        _ => false,
    }
}
```

### 2c. Apply `#[must_emit]` to all `on_event` implementations

Add `canon_proc_macros = { package = "canon-proc-macros", path = "../canon-proc-macros" }`
to `canon-runtime/Cargo.toml` and `canon-loop/Cargo.toml`, `canon-route/Cargo.toml`,
`canon-goodness/Cargo.toml` dependencies.

Then annotate every `on_event`:

```rust
// In each impl EventConsumer block:
use canon_proc_macros::must_emit;

#[must_emit]
fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
    match event {
        RuntimeEvent::Tick(_)        => { ... }
        RuntimeEvent::LoopRewarded(_) => { ... }
        // ALL 40+ variants listed explicitly — no _ arm
        RuntimeEvent::Code(_)
        | RuntimeEvent::Debug(_)
        | RuntimeEvent::Edit(_)
        | RuntimeEvent::LoopObserved(_)
        | RuntimeEvent::LoopPlanned(_)
        | RuntimeEvent::LoopActed(_)
        | RuntimeEvent::LoopVerified(_)
        | RuntimeEvent::GoodnessSnapshot(_)
        | RuntimeEvent::RouteTick(_)
        | RuntimeEvent::RouteSelected(_)
        | RuntimeEvent::Cargo(_)
        | RuntimeEvent::File(_)
        | RuntimeEvent::Bash(_)
        | RuntimeEvent::Llm(_)
        | RuntimeEvent::RequestDispatch(_)
        | RuntimeEvent::SubTaskResult(_)
        | RuntimeEvent::Analysis(_)
        | RuntimeEvent::RuntimeStateUpdated(_)
        | RuntimeEvent::NodeReady(_)
        | RuntimeEvent::NodeStarted(_)
        | RuntimeEvent::NodeCompleted(_)
        | RuntimeEvent::NodeFailed(_)
        | RuntimeEvent::CapabilityCompleted(_)
        | RuntimeEvent::CapabilityFailed(_)
        | RuntimeEvent::PolicyBaselineUpdated(_)
        | RuntimeEvent::GoalSelected(_)
        | RuntimeEvent::SystemConfigLoaded(_)
        | RuntimeEvent::AgentRegistered(_)
        | RuntimeEvent::PromptLoaded(_)
        | RuntimeEvent::ToolCall(_)
        | RuntimeEvent::ToolResult(_)
        | RuntimeEvent::ToolBatchSettled(_)
        | RuntimeEvent::GoalNodeCreated(_)
        | RuntimeEvent::GoalNodeRetracted(_)
        | RuntimeEvent::GoalNodeRewritten(_)
        | RuntimeEvent::GoalEdgeDefined(_)
        | RuntimeEvent::GoalGraphCheckpointed(_)
        | RuntimeEvent::CapabilityInvoked(_)
        | RuntimeEvent::ErrorOccurred(_)
        | RuntimeEvent::CapabilityResolved(_)
            => EventOutcome::NoOp("not a trigger for this consumer"),
    }
}
```

**Copy this exhaustive ignore block into every consumer.** When a new variant is added
to `RuntimeEvent`, every `#[must_emit]`-annotated function that doesn't list it will
fail with a standard Rust non-exhaustive match error — before `#[must_emit]` even runs.

### 2d. MIR-level verifier (canon-rustc — future phase)

The proc-macro above is AST-level. For a deeper guarantee — verifying that no
control-flow path reaches a function exit without passing through an `EventOutcome::Emit`
at least once per pipeline cycle — implement a custom lint in `canon-rustc`:

- Walk the MIR basic blocks of every function tagged `#[must_emit]`
- For each basic block that terminates the function (`TerminatorKind::Return`),
  check that the return value is not `EventOutcome::NoOp` unless the function
  carries the `#[allow(no_op_emit(reason = "..."]` attribute
- This requires the `canon-rustc` wrapper to register a `LateLintPass` and inspect
  `mir::Body`

This is a separate future plan. The proc-macro in 2c is sufficient for the current goal.

---

## Layer 3: `WatchdogConsumer` — stage-staleness detector + heartbeat

### 3a. New file: `canon-utils/canon-runtime/src/consumers/watchdog_consumer.rs`

```rust
use canon_event::{
    EventConsumer, EventFilter, EventOutcome, RuntimeEvent,
    new_error_occurred,
};
use std::collections::HashMap;
use canon_proc_macros::must_emit;

/// Staleness thresholds (ticks). A stage that has not fired within this many
/// ticks triggers a watchdog warning event.
const STAGE_THRESHOLDS: &[(&str, u64)] = &[
    ("observed",  10),
    ("planned",   15),
    ("acted",     15),
    ("verified",  20),
    ("rewarded",  25),
];

pub struct WatchdogConsumer {
    current_tick: u64,
    last_stage_tick: HashMap<&'static str, u64>,
}

impl WatchdogConsumer {
    pub fn new() -> Self {
        let mut last_stage_tick = HashMap::new();
        for (stage, _) in STAGE_THRESHOLDS {
            last_stage_tick.insert(*stage, 0u64);
        }
        Self { current_tick: 0, last_stage_tick }
    }
}

impl EventConsumer for WatchdogConsumer {
    fn filter(&self) -> EventFilter { EventFilter::All }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
        match event {
            RuntimeEvent::Tick(t) => {
                self.current_tick = t.tick;
                let mut stalled: Vec<(&str, u64)> = Vec::new();
                for (stage, threshold) in STAGE_THRESHOLDS {
                    let last = self.last_stage_tick.get(stage).copied().unwrap_or(0);
                    let idle = self.current_tick.saturating_sub(last);
                    if idle >= *threshold {
                        stalled.push((stage, idle));
                    }
                }
                if stalled.is_empty() {
                    return EventOutcome::NoOp("watchdog_all_stages_healthy");
                }
                // Emit one ErrorOccurred per stalled stage.
                let events: Vec<RuntimeEvent> = stalled.iter().map(|(stage, idle)| {
                    RuntimeEvent::ErrorOccurred(new_error_occurred(
                        "watchdog_stall",
                        "watchdog",
                        format!("Stage '{stage}' has not fired in {idle} ticks"),
                        "warning",
                        serde_json::json!({ "stage": stage, "idle_ticks": idle }),
                        None,
                    ))
                }).collect();
                EventOutcome::EmitMany(events)
            }

            RuntimeEvent::LoopObserved(_)  => { self.last_stage_tick.insert("observed", self.current_tick); EventOutcome::NoOp("watchdog_stage_reset") }
            RuntimeEvent::LoopPlanned(_)   => { self.last_stage_tick.insert("planned",  self.current_tick); EventOutcome::NoOp("watchdog_stage_reset") }
            RuntimeEvent::LoopActed(_)     => { self.last_stage_tick.insert("acted",    self.current_tick); EventOutcome::NoOp("watchdog_stage_reset") }
            RuntimeEvent::LoopVerified(_)  => { self.last_stage_tick.insert("verified", self.current_tick); EventOutcome::NoOp("watchdog_stage_reset") }
            RuntimeEvent::LoopRewarded(_)  => { self.last_stage_tick.insert("rewarded", self.current_tick); EventOutcome::NoOp("watchdog_stage_reset") }

            // All other variants — explicitly ignored by this consumer.
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::AgentRegistered(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
                => EventOutcome::NoOp("watchdog_not_a_stage_event"),
        }
    }
}
```

### 3b. Heartbeat thread — prevents Tick starvation

The watchdog consumer only fires when `Tick` events arrive. If the main event loop
blocks (filesystem watch stalls, channel blocks), no `Tick` fires and no watchdog
check runs. Add a separate heartbeat thread in `event_runtime.rs` that injects
`Tick` events on a wall-clock schedule if the main loop hasn't ticked recently.

**Add to `canon-utils/canon-runtime/src/bin/event_runtime.rs`**, just before
`let mut runtime = EventRuntime::new(consumers);`:

```rust
// Heartbeat thread: injects a Tick every 5 seconds via a shared atomic counter.
// Prevents the watchdog from going blind if the main event loop stalls.
let heartbeat_tick = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
{
    let heartbeat_tick = heartbeat_tick.clone();
    // We emit via the tlog path directly so the heartbeat works even if the
    // in-memory runtime's channel is blocked.
    let tlog_path = tlog_path.clone();
    std::thread::spawn(move || {
        use std::time::Duration;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let tick = heartbeat_tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Write a Tick event to tlog so the file-tail reader picks it up.
            // The runtime will then dispatch it to all consumers including WatchdogConsumer.
            let _ = canon_event::write_canon_event_auto(
                &tlog_path,
                &canon_event::CanonEvent {
                    event_id: None,
                    meta: canon_event::EventMeta {
                        ts: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0),
                        source: "heartbeat".to_string(),
                        file: String::new(),
                        line: 0,
                    },
                    payload: canon_event::CanonPayload::from_kind(
                        "Tick",
                        serde_json::json!({ "tick": tick }),
                    ),
                },
            );
        }
    });
}
```

### 3c. Register `WatchdogConsumer` in `event_runtime.rs`

```rust
// Add import:
use canon_runtime::consumers::watchdog_consumer::WatchdogConsumer;

// Add to consumers vec (after GoalGenConsumer and AnalystConsumer):
Box::new(WatchdogConsumer::new()),
```

### 3d. Add `pub mod watchdog_consumer;` to `consumers/mod.rs`

---

## Summary: what compile-time enforces after this plan

| Scenario | Before | After |
|---|---|---|
| New `RuntimeEvent` variant added | Silently ignored by all consumers | Compile error in every `#[must_emit]` consumer that hasn't listed it |
| Consumer returns from `on_event` without declaring outcome | Silent — returns `()` | Compile error — return type is `EventOutcome` |
| `NoOp` without a reason | Allowed silently | Requires `&'static str` reason — visible in code review |
| Wildcard `_ =>` in event match | No restriction | `#[must_emit]` macro rejects it at compile time |
| Pipeline stage stalls at runtime | No detection | `WatchdogConsumer` emits `ErrorOccurred("watchdog_stall")` after threshold ticks |
| Main loop tick starvation | Watchdog never fires | Heartbeat thread injects `Tick` on wall clock |

## Execution order for codex

1. Apply **Prerequisite P1–P3** (config.toml + diagnostics fixes)
2. Define **`EventOutcome`** + change trait signature + update `EventRuntime` loop
3. Migrate **all consumers** to return `EventOutcome`; add tick timeouts to `GoalGenConsumer` and `AnalystConsumer` `Pending` states
4. Create **`canon-proc-macros`** crate with `#[must_emit]`; add to workspace
5. Annotate all `on_event` with `#[must_emit]`; verify build passes
6. Add **`WatchdogConsumer`** + heartbeat thread + register in `event_runtime.rs`
7. Run `cargo check -p canon-runtime` to confirm zero errors/warnings
