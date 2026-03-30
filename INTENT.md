# INTENT
## Objective
Break the infinite planning loop caused by LLM planner timeouts by tracking consecutive failures and routing to observe after a threshold instead of repeatedly selecting plan.
## Constraints
- no build break
- no test failure
## Targets
- canon-utils/canon-route/src/context.rs
- canon-utils/canon-route/src/executor.rs
## Success Criteria
- consecutive LLM planning failures are incremented on llm_failed or timeout and reset on successful planning
- routing switches to observe when failures ≥ 2
- router_disabled_fallback returns observe instead of always selecting plan
- infinite plan → plan loop is eliminated in runtime behavior
- agent execution progresses past repeated LLM timeout scenarios
