# Canon Implementation Plan

## Pipeline Invariant

```
Capture -> CanonIR -> Graph -> Solve -> Emit
```

## Current Status

- Solve stage: GPU algorithms wired into solvers. Complete.
- Emit stage: Correct and complete. Panics on bad IR are intentional guards.
- Capture stage: Three structural gaps producing broken IR.

## Active Error Count

12 errors across 4 files in emitted output.
Categories: E0277 (5), E0308 (4), E0599 (3).

---

## Gap 1 — Iterator Loop Bodies Not Lowered

**Severity:** Critical. Affects every function using `for x in iter`.

**Root cause:** `analyze_switch_structure` in `canon-capture/src/capture/mir/analysis.rs`
does not recognize the MIR iterator loop pattern:
```
loop {
  _next = iter.next();
  match _next {            <- SwitchInt on Option discriminant
    Some(x) => { body }   <- arm 1: loop body blocks
    None    => { break }  <- arm 0: exit block
  }
}
```
The body blocks are present in MIR but never emitted into the IR.

**Algorithm now available:** `algorithms::control_flow::cfg_pattern`
- `compute_back_edges(adj)` -> `HashSet<(usize,usize)>`
- `detect_iterator_loops(adj, back_edges)` -> `Vec<IteratorLoopPattern>`
- Each pattern carries: `loop_head`, `switch_block`, `body_entry`, `exit_block`, `body_blocks`

**Fix location:** `canon-capture/src/capture/mir/analysis.rs`

**Steps for agent:**
1. After building the CFG adj from MIR blocks, call `compute_back_edges`.
2. Call `detect_iterator_loops` to get all loop patterns.
3. For each `IteratorLoopPattern`, mark `body_blocks` as `BlockRole::Normal`
   so `stage_emit_draft` processes them.
4. In `SwitchAnalysis`, record that the switch is an iterator switch so
   `lower_non_call_terminator` emits a `Goto` to body_entry instead of pruning.

**Expected outcome:** Loop bodies emit. `extract_top_level`, `collect_struct_fields`,
`collect_enum_variants`, `collect_methods` all recover their loop contents.

---

## Gap 2 — Closure Arguments Collapse to `()`

**Severity:** High. Affects all `.map()`, `.filter()`, `.and_then()` call sites.

**Root cause:** In MIR, a closure passed to `.map()` is a `Rvalue::Closure` or
a function-pointer coercion. `mir_operand_label` in
`canon-capture/src/capture/mir/ops.rs` returns `None` for these operands,
which propagates as `()` into the call argument list.

Emitted symptom:
```rust
let mut _v6: () = panic!("canon missing assignment lowering");
__ret = _v4.map(_v6);   // E0277: expected FnOnce, found ()
```

**Algorithm now available:** `algorithms::control_flow::use_def`
- `build_use_def_chains(adj, facts)` -> `UseDefChains`
- Tracks which definition of a local reaches each use site.
- Allows capture layer to confirm a local holds a closure before emitting it.

**Fix location:** `canon-capture/src/capture/mir/ops.rs` + `terminator.rs`

**Steps for agent:**
1. In `mir_operand_label`: when operand is `Operand::Copy(place)` or `Operand::Move(place)`
   and the local's type in MIR is a closure or fn-ptr, return `Some(resolver.label_place(place))`.
   Do not return `None` for these — emit the local name directly.
2. In `lower_call_terminator`: when `mir_call_args_labels` returns `None` for an argument,
   fall back to the MIR local index name (`_vN`) rather than suppressing the argument.
3. Remove the suppression path that converts closure argument failures to `()`.

**Expected outcome:** `.map(_v6)` becomes `.map(closure_local)` where
`closure_local` is a properly named local holding the closure value.

---

## Gap 3 — Format/Runtime Calls Collapse to `()`

**Severity:** Medium. Affects functions using `format!`, `write!`, `println!`.

**Root cause:** `is_format_call_target` in `canon-capture/src/capture/mir/ops.rs`
filters format macro expansion calls. The destination local receives `()` as
a sentinel. When that local flows into `__ret`, the return type becomes `()`.

Emitted symptom:
```rust
let mut _v19: () = panic!("canon internal fmt/runtime call");
let mut __ret = _v18;   // E0308: expected String, found ()
```

**Fix location:** `canon-capture/src/capture/mir/terminator.rs`
`lower_call_terminator` lines 13-116.

**Steps for agent:**
1. When `filtered_internal_call_target` returns true and destination is `Some(place)`,
   emit a typed `Default::default()` assignment to the destination local instead of
   propagating `()`. Use `emit_suppressed_for_name` with a `Default::default()` expr.
2. Do not emit `panic!("canon internal fmt/runtime call")` — emit
   `::core::default::Default::default()` so the binding has the correct type shape.
3. Add a special case: if the filtered call destination is the return place (`_0` in MIR),
   skip the assignment entirely and let the existing return place handling manage it.

**Expected outcome:** Format-heavy functions emit with typed placeholders instead of
unit sentinels. E0308 errors from format call sites eliminated.

---

## Solve Stage — Type Solver Writeback (uses GPU AC-3 result)

**Severity:** Medium. Closes unit-type pollution in Local nodes.

**Root cause:** `type_solver.rs` runs AC-3 and detects empty domains but discards
the result with a `WARN` log. The pruned domain information needs to write back
into IR node `TypeKind` fields so projection sees resolved types.

**Algorithm available:** `algorithms::constraints::ac3::ac3_gpu_apply` (already wired).
**Additional algorithm:** `algorithms::control_flow::interval_analysis::interval_narrowing`
for numeric type bounds beyond equality.

**Fix location:** `canon-analyzer/src/solver/type_solver.rs`

**Steps for agent:**
1. After `ac3_gpu_apply` returns `pruned` domains, for each variable where
   `pruned[var].len() == 1`, the type is fully resolved.
2. Get the concrete `CanonId` from `pruned[var][0]` and the original node index
   from `var_to_node[var]`.
3. If `ir.nodes[node_idx].kind` is `Type { kind: TypeKind::Unresolved(..) }`
   or `Type { kind: TypeKind::Param(..) }`, replace it with
   `Type { kind: TypeKind::Adt(CanonId(resolved_id as u32)) }`.
4. Update `ir.type_index` to reflect the replacement.

---

## New Algorithms Added (available for future use)

All files are in `algorithms/src/`.

| Algorithm                                    | File                                | Purpose in Canon                             |
|----------------------------------------------+-------------------------------------+----------------------------------------------|
| `cfg_pattern::detect_iterator_loops`         | `control_flow/cfg_pattern.rs`       | Gap 1: iterator loop recognition in capture  |
| `cfg_pattern::compute_back_edges`            | `control_flow/cfg_pattern.rs`       | Back-edge detection for loop structure       |
| `cycle_report::topological_sort_with_cycles` | `graph/cycle_report.rs`             | dep_solver cycle diagnostics                 |
| `interval_analysis::interval_narrowing`      | `control_flow/interval_analysis.rs` | Type domain narrowing beyond AC-3            |
| `use_def::build_use_def_chains`              | `control_flow/use_def.rs`           | Gap 2: closure local tracking in capture     |
| `worklist::forward_worklist`                 | `control_flow/worklist.rs`          | Generic dataflow fixed-point for any solver  |
| `worklist::backward_worklist`                | `control_flow/worklist.rs`          | Backward dataflow (liveness, live variables) |

---

## Execution Order for Agent

```
Gap 1 (iterator loops)  ->  cargo check -p canon-capture
Gap 2 (closure args)    ->  cargo check -p canon-capture
Gap 3 (format calls)    ->  cargo check -p canon-capture
Solve writeback         ->  cargo check -p canon-analyzer
Run pipeline            ->  check error count drops
```

Gap 1 first — it recovers the most code. Gaps 2 and 3 are independent and
can be done in either order after Gap 1.

---

## Round-Trip Invariant (long-term target)

```
Source -> Capture -> CanonIR -> Solve -> Emit -> Source'
Source == Source' modulo formatting
```

This requires all three gaps closed plus type solver writeback.
Current state: Emit is correct. Solve is correct. Capture has three gaps.
