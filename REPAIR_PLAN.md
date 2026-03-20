# REPAIR_PLAN.md — Kernel Event Routing Off Main Thread

## Root Cause (confirmed via tlog analysis)

`bus.dispatch` is **already non-blocking**. Each consumer runs in its own dedicated thread
(`EventBus.register` spawns a thread + crossbeam bounded channel per consumer). `bus.dispatch`
sends to channels; it does not call `on_event` synchronously.

The real blocking is in `drain_event_queue_with_grace` inside `handle_control_msg`.
After each LLM routing call completes, the main loop drains `q_event_rx` to process
any events that arrived during the call. This processes every event from both:
- **P1** (bootstrap replayer, line 715-717 of event_runtime.rs)
- **P2** (inotify watcher, line 765-769 of event_runtime.rs)

For each event, `handle_event_msg` → `runtime.process_events()` →
`handle_kernel_event()` is called. Each kernel event does:
1. `apply_delta(&mut self.state, &delta)` — for EdgeDefined/EdgeRemoved events, this calls
   `known_edges.retain(...)` on a `Vec`, which is **O(n)** where n is the current edge count.
   With 67,012 edge events and a growing graph, this is O(n²) total.
2. `handle_runtime_event(CanonEvent::Code(Code { delta, state: self.state.clone() }))` —
   **clones the entire `RustcState`** (HashMap + Vec + HashSet) for every single event,
   then tries-sends to all consumer channels (fast), then fails to match in `append_runtime_event`
   (Code has no write arm — it is not written to the tlog by the runtime).

**Confirmed in tlog**: 184,511 watcher events between `tick=6 signals_snapshot` and
`verify_result` at index [196,606]. All from watcher: 97,595 rustc_events, 67,012
dependency_edge, 19,819 symbol_emitted, 43 crate_compiled, 42 file_processed.
The routing loop could not advance to tick=7 because `drain_event_queue_with_grace`
was processing all 184,511 of them.

---

## Fix — Route kernel events to a dedicated background thread

Kernel events (watcher rustc events) are **irrelevant to routing**. The routing loop
only needs:
- `CapabilityRequested("analysis.run" / "analysis.workspace")` when `CompilationUnitFinished`
- `RuntimeStateUpdated(workspace_dirty=true)` when compilation units change

Both of these can be **emitted by a background kernel processor thread** via the
shared `EventEmitterHandle`. The main routing loop never needs to call `apply_delta`
or clone `RustcState` for 184,511 watcher events.

No consumer registered in `main()` responds to `CanonEvent::Code` events:
- `ObserveConsumer`, `PlanConsumer`, `ActConsumer`, `VerifyConsumer`, `RewardConsumer`:
  all match on LoopObserved / LoopActed / LoopVerified / Debug / CapabilityCompleted
- `ErrorLogger`: `EventFilter::ErrorOnly` — never sees Code
- `CapabilityExecutor`: `EventFilter::CapabilityOnly` — never sees Code

Code events dispatched to consumers use `try_send` (non-control events), so they are
already dropped when consumer queues are full. Stopping Code dispatch from kernel events
has zero behavioural impact on the running system.

---

## File: `canon-utils/canon-runtime/src/lib.rs`

### Addition 1 — `emitter_handle()` method on `EventRuntime`

Add after the existing `next_id()` method (around line 94):

```rust
pub fn emitter_handle(&self) -> EventEmitterHandle {
    self.emitter.clone()
}
```

### Addition 2 — `spawn_kernel_processor` free function

Add at the bottom of `lib.rs`, below `apply_delta`:

```rust
// ---------------------------------------------------------------------------
// Kernel processor — processes watcher (rustc plugin) events off the main
// routing loop thread.  Owns its own RustcState.  Emits CapabilityRequested
// back to the main loop via the shared emitter handle when a
// CompilationUnitFinished event is observed.
// ---------------------------------------------------------------------------

pub fn spawn_kernel_processor(
    rx: crossbeam_channel::Receiver<KernelMsg>,
    emitter: EventEmitterHandle,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("canon-kernel-processor".to_string())
        .spawn(move || {
            let mut state = empty_state();
            let mut tick: u64 = 0;
            for msg in rx.iter() {
                match msg {
                    KernelMsg::Reset => {
                        state = empty_state();
                        tick = 0;
                    }
                    KernelMsg::Event(event) => {
                        let AnyEvent::Canon(ref canon) = event else { continue };
                        let Some(kernel) = extract_rustc_event(canon) else { continue };
                        let crate_name = if let RustcEvent::CompilationUnitFinished(ref cu) = kernel {
                            Some(cu.crate_name.clone())
                        } else {
                            None
                        };
                        if matches!(kernel, RustcEvent::SessionStart(_)) {
                            state = empty_state();
                            tick = 0;
                        } else {
                            tick = tick.saturating_add(1);
                        }
                        let delta = EventDelta { id: tick, tick, event: kernel };
                        let _ = apply_delta(&mut state, &delta);
                        if let Some(crate_name) = crate_name {
                            emitter.emit(CanonEvent::CapabilityRequested(CapabilityRequested {
                                request_id: format!("analysis-k-{}-{}", crate_name, tick),
                                name: "analysis.run".to_string(),
                                args: serde_json::json!({ "crate": crate_name }),
                            }));
                            emitter.emit(CanonEvent::CapabilityRequested(CapabilityRequested {
                                request_id: format!("analysis-workspace-k-{}", tick),
                                name: "analysis.workspace".to_string(),
                                args: serde_json::json!({}),
                            }));
                        }
                    }
                }
            }
        })
        .expect("kernel processor thread")
}

pub enum KernelMsg {
    Event(canon_event_store::AnyEvent),
    Reset,
}
```

**Required imports to add** (already present in `lib.rs` scope, verify each):
- `RustcEvent` — already imported via `canon_event`
- `extract_rustc_event` — already imported via `canon_event_store`
- `CapabilityRequested`, `EventDelta`, `EventEmitterHandle` — already imported
- `AnyEvent` — already imported via `canon_event_store`

---

## File: `canon-utils/canon-runtime/src/bin/event_runtime.rs`

### Change 1 — Import `KernelMsg` and `spawn_kernel_processor`

Add to the existing `use canon_runtime::...` import:

```rust
// BEFORE:
use canon_runtime::{register_default_capabilities, EventRuntime};

// AFTER:
use canon_runtime::{register_default_capabilities, spawn_kernel_processor, EventRuntime, KernelMsg};
```

### Change 2 — Helper predicate `is_kernel_canon_event`

Add as a free function near the top of `event_runtime.rs` (before `main`):

```rust
fn is_kernel_canon_event(event: &AnyEvent) -> bool {
    if let AnyEvent::Canon(canon) = event {
        extract_rustc_event(canon).is_some()
    } else {
        false
    }
}
```

### Change 3 — Create `q_kernel_tx/rx` channel alongside `q_event_tx/rx`

After the existing channel creation at line 710:

```rust
// EXISTING:
let (q_control_tx, q_control_rx) = cc::unbounded::<ControlMsg>();
let (q_event_tx, q_event_rx) = cc::unbounded::<EventMsg>();

// ADD immediately after:
let (q_kernel_tx, q_kernel_rx) = cc::unbounded::<KernelMsg>();
```

### Change 4 — Spawn the kernel processor thread

After the `runtime.set_next_id(resumed_next_id)` setup block and before the `once` mode
early return (so it's only spawned for the live runtime, not once mode):

```rust
// Spawn kernel processor before P1/P2 producers send events.
let kernel_emitter = runtime.emitter_handle();
let _kernel_processor = spawn_kernel_processor(q_kernel_rx, kernel_emitter);
```

Place this immediately after:
```rust
runtime.set_execute_capabilities(false);
runtime.set_tlog_path(tlog_path.clone());
runtime.set_next_id(resumed_next_id);
// ← INSERT HERE, before the `if once { ... }` block
```

### Change 5 — P1 bootstrap replayer: route kernel events to `q_kernel_tx`

```rust
// BEFORE (line 715-717):
for event in bootstrap_events.into_iter().skip(processed) {
    q_event_tx.send(EventMsg::Event(event)).ok();
}

// AFTER:
for event in bootstrap_events.into_iter().skip(processed) {
    if is_kernel_canon_event(&event) {
        q_kernel_tx.send(KernelMsg::Event(event)).ok();
    } else {
        q_event_tx.send(EventMsg::Event(event)).ok();
    }
}
```

### Change 6 — P2 watcher: route kernel events to `q_kernel_tx`

The P2 watcher thread needs `q_kernel_tx`. Clone it before the thread spawn:

```rust
// ADD before the P2 thread spawn block:
let kernel_tx = q_kernel_tx.clone();
```

Inside the P2 thread, update the new-event delivery loop (lines 765-770):

```rust
// BEFORE:
for event in all.into_iter().skip(watcher_seen) {
    watcher_seen += 1;
    if watcher_tx.send(EventMsg::Event(event)).is_err() {
        break;
    }
}

// AFTER:
for event in all.into_iter().skip(watcher_seen) {
    watcher_seen += 1;
    if is_kernel_canon_event(&event) {
        if kernel_tx.send(KernelMsg::Event(event)).is_err() {
            break;
        }
    } else if watcher_tx.send(EventMsg::Event(event)).is_err() {
        break;
    }
}
```

Also update the Reset path (lines 758-761) to reset the kernel processor's state
in addition to sending the full Reset to the main loop:

```rust
// BEFORE:
if watcher_tx.send(EventMsg::Reset(all)).is_err() {
    break;
}

// AFTER:
if kernel_tx.send(KernelMsg::Reset).is_err() {
    break;
}
// Replay kernel events into processor from the reset payload before sending
// control Reset so that processor state is rebuilt before the main loop
// re-evaluates routing signals.
for event in &all {
    if is_kernel_canon_event(event) {
        kernel_tx.send(KernelMsg::Event(event.clone())).ok();
    }
}
if watcher_tx.send(EventMsg::Reset(all)).is_err() {
    break;
}
```

Note: On Reset, the main loop calls `runtime.reset()` and `runtime.process_events(all)`.
`process_events` still has the `extract_rustc_event` branch — it will try to call
`handle_kernel_event` for kernel events in the Reset payload. Since these are now also
routed to the kernel processor, this causes duplicate analysis triggers.
To prevent this, update the Reset handling in `handle_event_msg` to filter kernel events
out of the Reset payload before passing to `process_events`:

```rust
// In handle_event_msg, the Reset arm:
EventMsg::Reset(events) => {
    runtime.reset();
    let non_kernel: Vec<AnyEvent> = events
        .iter()
        .filter(|e| !is_kernel_canon_event(e))
        .cloned()
        .collect();
    runtime.process_events(&non_kernel)?;
    apply_observed_events(runtime, route_state, workspace)?;
    *processed = events.len();
}
```

---

## What does NOT change

- `EventBus`, `bus.dispatch`, consumer threads — unchanged. Already non-blocking.
- `VerifyConsumer.verify_acted` / `run_cargo_check` — already runs in its own consumer
  thread (spawned by `bus.register`). Does not block the main routing thread.
- `CapabilityExecutor.on_event` — already in its own thread. Returns fast for `Deferred`.
- `analysis.run` / `analysis.workspace` capabilities — already async background thread
  (previous fix). Unchanged.
- `LlmCapabilityHandler` — unchanged.
- `EventRuntime.handle_kernel_event` and `EventRuntime.state` — still present for
  the `once` mode code path and internal state tracking. Just not called from the
  main routing drain anymore (no kernel events reach `q_event_rx`).
- `apply_delta` — unchanged. Now called by kernel processor thread only.
- Analysis CapabilityRequested events — still written to tlog (when the main loop
  drains `emitter_rx` via `drain_emitted_events`, it calls `handle_runtime_event`
  → `append_runtime_event` for CapabilityRequested).
- `crate_compiled` old-format events — these are custom events (not rustc_events),
  `is_kernel_canon_event` returns false for them; they continue through `q_event_tx`
  and are processed by `process_events` as before. Note: these trigger analysis.run
  in `process_events`; combined with the kernel processor also triggering on
  `CompilationUnitFinished`, there may be duplicate analysis requests for modern
  rustc events. The `RUN_GUARD` dedup in `runner.rs` already handles this.

---

## Expected behaviour after fix

```
Tick fires (P3)
  → handle_control_msg
  → request_route_via_llm_call  (90s timeout, main thread blocks on LLM)

  [meanwhile P2 watcher writes 184,511 kernel events to q_kernel_tx]
  [kernel processor thread processes all 184,511 events independently]
  [kernel processor emits 86 CapabilityRequested(analysis.*) to emitter_rx]

  → LLM returns
  → drain_event_queue_with_grace(Duration::ZERO)
      q_event_rx is EMPTY (kernel events never entered q_event_rx)
      drains only emitted events: 86 CapabilityRequested from kernel processor
      each: send Deferred to analysis background thread, return immediately
  → signals computed, gate applied, route_selected emitted to tlog
  → tick=7 fires
```

`drain_event_queue_with_grace` processes ~100 events per tick (emitter events and
routing control events) instead of 184,511. Per-tick routing latency drops from
minutes to milliseconds.

---

## File change summary

| File | Change |
|---|---|
| `canon-utils/canon-runtime/src/lib.rs` | Add `pub fn emitter_handle()`. Add `pub enum KernelMsg`. Add `pub fn spawn_kernel_processor(rx, emitter)`. |
| `canon-utils/canon-runtime/src/bin/event_runtime.rs` | Add `is_kernel_canon_event`. Create `q_kernel_tx/rx`. Spawn kernel processor. P1 + P2 producers route kernel events to `q_kernel_tx`. Reset arm filters kernel events from `process_events` payload. |
