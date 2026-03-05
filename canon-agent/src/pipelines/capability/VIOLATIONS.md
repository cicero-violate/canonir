# MODULE DEPENDENCY VIOLATIONS

## Layer Model

```

types → graph → planner → policy → scheduler → engine → io

```

Rule:

```

lower_layer → higher_layer = violation

```

---

# Current Status

No active violations detected in code. Previous issues have been resolved:

- Planner/Templates cycle resolved by moving `PlannerUpdate` and `apply_planner_update` into `planner_update.rs`.
- Planner and Types no longer depend on IO.
- Scheduler and Execution no longer import IO directly.

The remaining dependency `engine → llm → endpoint_worker → tab_management` is intentional and matches the target stack where IO is accessed only through the engine layer. IO worker initialization and tab handle creation are now routed through `engine::init_io_workers` and `engine::new_tabs`.
