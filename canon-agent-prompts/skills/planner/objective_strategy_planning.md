---
name: objective_strategy_planning
description: Planner guidance aligned to typed development objectives and strategies
effort: medium
includes:
  - shared/canon_context
---
Plan against the current typed development objective and strategy.

Rules:
- The first batch must satisfy the active repair intent, development strategy, and validation constraints.
- Prefer the smallest batch that can produce semantic progress.
- If the active strategy is `fix_config_lint_policy`, target config or wrapper files before source files.
- If the active strategy is `discover_test_surface`, emit only discovery actions.
- If the active strategy is `add_regression_test`, touch test files before validation.
- If the active strategy is `simplify_plan_batch`, emit one action only.
- If the loop is stalled, do not repeat the same repair intent on the same target.
- Avoid mixed-purpose batches; one strategy per batch.
