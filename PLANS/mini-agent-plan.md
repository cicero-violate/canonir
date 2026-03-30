Fix: Enforce PlanningCompleted → Act at Policy Layer
Problem
The system violates the control-flow invariant:

PlanningCompleted → RouteSelected(act)
Observed behavior from logs:

PlanningCompleted → RouteSelected(observe) ❌
This occurs because state-based heuristic rules (e.g. no_actionable_failure → observe) override event-driven transitions, causing invalid routing after planning completes.

Root Cause
In canon-utils/canon-route/src/policy.rs:

evaluate_route_transition(...) allows heuristic rules to fire after PlanningCompleted

No hard precedence rule exists for this event

As a result, policy emits Observe instead of Act

This is a policy-layer bug, not an executor issue.

Correct Invariant
∀ e:
e == PlanningCompleted ⇒ next = Act
This must override all heuristic logic.

Fix
1. Enforce Event Priority
Update evaluate_route_transition to short-circuit on PlanningCompleted:

// HARD invariant: PlanningCompleted must always transition to Act
if let Some(RuntimeEvent::PlanningCompleted(_)) = event {
    if ctx.pending_tool_result_ids.is_empty() {
        return RouteTransitionEvaluation {
            rules: vec![RouteProposal::PlannedToAct],
            ..Default::default()
        };
    }
}
2. Remove Conflicting Rules
Delete or bypass rules like:

no_actionable_failure → observe
when the current event is PlanningCompleted.

Why This Fix Is Correct
Correct layering
Layer	Responsibility
policy.rs	decides valid transitions
executor.rs	enforces mechanics only
Fixing executor introduces symptom masking, not correctness.

Expected Behavior After Fix
Before
PlanningCompleted
→ RouteSelected(observe) ❌
→ LoopObserved
After
PlanningCompleted
→ RouteSelected(act) ✅
→ LoopActed
Additional Cleanup
Remove executor workaround:

// REMOVE THIS
if fallback.route == "observe" && failure_class == "no_actionable_failure" {
    return;
}
Key Principle
event-driven invariants > heuristic state rules
Result
Restores FSM correctness

Eliminates invalid Plan → Observe transitions

Ensures deterministic loop progression

Prevents silent invariant violations

Add a unit test
