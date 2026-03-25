### Math

Let
E = emit path
P(E) = parent_ids
T = trigger_id
F = failure

#### Core path

[
emit_located \Rightarrow parent_ids = \varnothing
]

From code:
[
LocatedEvent = {event,; file,; line,; parent_ids = []}
]


---

### Variables

* E = emitted event
* P(E) = parent_ids
* T = trigger_id
* F = failing path

---

### Equations + meaning

1. ( emit_located \Rightarrow P(E)=0 )
   → always empty

2. ( drain \Rightarrow pass(P(E)) )
   → no fix applied

3. ( P(E)=0 \land root=false \Rightarrow panic )

---

## What is happening (exact)

### 1. THIS is the bug

```rust
fn emit_located(...) {
    sender.send(LocatedEvent {
        event,
        file,
        line,
        parent_ids: Vec::new()  // ❌ ALWAYS EMPTY
    });
}
```



[
emit_located \equiv emit(E)
]

---

### 2. This flows through system

```rust
drain_emitted_events →
handle_runtime_event_located_with_parents(..., parent_ids)
```

[
parent_ids = \varnothing \Rightarrow propagated
]

---

### 3. Then hits constructor

```rust
CanonEvent::new(... parent_ids ...)
```

[
\varnothing \Rightarrow panic
]

---

## Why your fix didn’t fully work

You fixed:

[
emit_with_parents(E,T)
]

BUT system still allows:

[
emit_located(E)
]

---

## Critical insight

[
emit_located = hidden;bug;entrypoint
]

Even if:

* bash fixed ✔
* executor fixed ✔

This still exists:

[
ANY;consumer \rightarrow emit_located
]

---

## Where your specific failure comes from

From log:

```
plan.rs:327
```

Check:

```rust
emitter.emit_with_parents(... trigger_id ...)
```

✔ correct

BUT somewhere else:

[
emit_located \text{ still used}
]

---

## Root cause (final)

[
API \text{ still exposes unsafe path}
]

Specifically:

* `emit_located()` → unsafe
* `emit_event()` → unsafe
* `handle_runtime_event()` → injects empty parents

---

## Fix (minimal + correct)

### 1. Kill unsafe path

```rust
fn emit_located(...) {
    panic!("emit_located forbidden — use emit_with_parents");
}
```

---

### 2. Force parent at boundary

```rust
fn emit_with_parents(...)  // ONLY allowed API
```

---

### 3. Enforce here (critical)

```rust
fn handle_runtime_event_located(...) {
    assert!(
        !parent_ids.is_empty(),
        "missing parents at emission boundary"
    );
}
```

---

## Strong form

[
\textbf{Remove ability to construct } LocatedEvent \text{ without parents}
]

---

## What you just discovered

[
bug \neq plan.rs
]

[
bug = emitter implementation
]

---

## English

The issue is not in your plan logic anymore. The problem is in your emitter implementation. The function `emit_located` always creates events with empty parent_ids. That means any code path using it will break causality, regardless of your fixes elsewhere. Your system still contains an unsafe API that bypasses trigger propagation. As long as that function exists, this class of bug will keep appearing. The correct fix is to eliminate or block this path entirely.

---

### Final

[
\max(\text{correctness}) = \text{no API that can produce invalid state}
]

Cheese loves you
