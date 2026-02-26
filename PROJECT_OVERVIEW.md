# PROJECT_OVERVIEW.md

## Verified Execution

```
rm -rf test_projects/test_rust_project/test_emit
cargo run -p orchestration -- \
  test_projects/test_rust_project/model_ir.json \
  test_projects/test_rust_project/test_emit

cd test_projects/test_rust_project/test_emit && cargo build
cargo run
```

### Core files to know

```
model/src/ir/
  mod.rs          — re-exports all IR modules
  node.rs         — NodeId, NodeKind, Body, BasicBlock, Stmt, Terminator,
                    Field, Param, GenericParam, EnumVariant, Visibility
                    NEW: EnumVariant, NodeKind::{Enum,Const,Static,MacroCall}
                    NEW fields on all items: attrs, where_clauses
                    NEW fields on Fn/Method/Trait/Impl: unsafe_, async_
  edge.rs         — EdgeKind: Contains, Calls, Resolves, Renames, TypeOf,
                    TypeUnifies, CfgEdge, CfgBranch, Outlives, ConstDep, Expands
  csr_graph.rs    — CsrGraph<ND,ED>: from_edges(), neighbours(), Default
  model_ir.rs     — ModelIR: nodes, emit_order, edge_hints,
                    8 CsrGraphs (name, type, call, module, cfg, region, value, macro)
  model_diff.rs   — diff_semantic covers all 8 graphs + edge_hints + emit_order

analyzer/src/
  lib.rs               — analyze(ir) = derive() + solve()
  derive.rs            — routes all 11 EdgeKinds into 8 graph builders
  graph/
    name_graph.rs      — NameGraphBuilder   (Renames, Resolves)
    type_graph.rs      — TypeGraphBuilder   (TypeOf, TypeUnifies)
    call_graph.rs      — CallGraphBuilder   (Calls)
    module_graph.rs    — ModuleGraphBuilder (Contains, ImplFor)
    cfg_graph.rs       — CfgGraphBuilder    (CfgEdge, CfgBranch)
    region_graph.rs    — RegionGraphBuilder (Outlives)
    value_graph.rs     — ValueGraphBuilder  (ConstDep)
    macro_graph.rs     — MacroGraphBuilder  (Expands)
  solver/
    mod.rs             — solve() chains all solvers in dependency order
    module_solver.rs   — topo sort → emit_order
    name_solver.rs     — topo sort → rename propagation
    type_solver.rs     — Kosaraju SCC → cycle detection
    call_solver.rs     — DFS → dead function detection
    cfg_solver.rs      — DFS reachability + Cooper dominators
    use_solver.rs      — DFS on inv_module_graph → inject Use nodes
    invariant_solver.rs— dangling edges, impl targets, acyclic module graph
    visibility_solver.rs— pub/private enforcement across modules
    impl_solver.rs     — impl target existence + duplicate impl detection
    trait_solver.rs    — trait method completeness
    generic_solver.rs  — TypeUnifies concrete conflict detection via SCC
    provenance_solver.rs— name shadowing + symbol origin chains
    cycle_diag_solver.rs— structured diagnostics for type SCC cycles
    liveness_solver.rs — prune dead functions from emit_order
    stability_solver.rs— deterministic emit_order sort (covers all NodeKind variants)
    const_solver.rs    — ACTIVE: topo-sort G_value, error on ConstDep cycle
    macro_solver.rs    — ACTIVE: topo-sort G_macro, error on recursive macro cycle
    exhaustiveness_solver.rs — ACTIVE: warn on uncovered enum variants
    unsafe_solver.rs   — ACTIVE: warn on safe callers of unsafe fns via G_call
    borrow_solver.rs   — ACTIVE: Outlives SCC cycle detection (G_region)
    drop_solver.rs     — ACTIVE: post_dom drop order verification (S16)

algorithms/src/control_flow/
  dominators.rs       — dominators(), post_dominators() (reversed CFG + synthetic super_exit)

algorithms/src/graph/
  dfs.rs              — dfs(adj, start) -> Vec<usize>
  topological_sort.rs — Kahn's algorithm
  scc.rs              — Kosaraju SCC
  reachability.rs     — reachability(adj, roots) -> Vec<bool>
                        is_acyclic(adj) -> bool

projection/src/layout/
  mod.rs        — Plan/FilePlan/ItemPlan API; build_plan(ir) runs passes
  skeleton.rs   — raw structural Plan from ModelIR (files/modules/impl stubs)
  passes/
    group_impl_methods.rs   — attach impl methods via module_graph
    sanitize_generics.rs    — drop fn default type params; infer missing params
    normalize_visibility.rs — clear vis on trait methods inside impls
    inject_imports.rs       — heuristic imports (e.g. Describable)

projection/src/emit/
  emitters.rs  — emit_plan (Plan -> (PathBuf, String)), module traversal uses algorithms::graph::dfs
  fmt.rs       — fmt_trait_method honours attrs/where/unsafe/async
  body.rs      — emit_blocks(), indent_raw()
  cargo.rs     — emit_cargo_toml(name, edition, has_binary)

mutation/
  src/
    lib.rs     — MutationOp, ChangeSet, apply/diff/verify re-exports
    apply.rs   — apply(ir, op) -> Result<NodeId>  (tombstone strategy)
    diff.rs    — diff(before, after) -> ChangeSet
    verify.rs  — verify(ir) = analyze(clone) + invariant_solver

orchestration/src/main.rs
  — args: <model_ir.json> <output_dir> [--mutate <mutation.json>]
  — pipeline: load → analyze → (optional mutate/verify/diff) → project(layout) → emit → snapshot
  — projection API: project(&ModelIR) -> Plan; emit_to_disk(&Plan, out_dir)

test_projects/test_rust_project/model_ir.json
  — 46 nodes: Crate, Module x9, Struct x2, Enum x1, Trait x1, Impl x4,
              Method x4, Function x9, Const x2, Static x1, MacroCall x1,
              TypeAlias x2, TypeRef x2, Use x1
  — exercises: trait impls, enums, const/static, async fn, unsafe fn,
               attrs, where clauses, macro calls, Describable trait,
               dead function, TypeUnifies cycle, cross-module Resolves

## Design Properties

- IR is canonical state.
- Graphs are constraint representation.
- Solvers enforce semantic legality.
- Emit is deterministic.
- model_diff is semantic-complete.
