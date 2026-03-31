Nice — this is exactly the right moment to tighten the system instead of fighting it.

Here are high-impact, immediately implementable changes that will make your executor feel 10x less painful:

🔧 1. Add a “no-progress watchdog”
Right now it happily loops idle forever.

Fix:
Track progress per step:

if last_actions.iter().all(|a| a == "run_command:echo idle") {
    idle_streak += 1;
} else {
    idle_streak = 0;
}

if idle_streak >= 3 {
    return Err("stuck: no progress in 3 steps".into());
}
👉 This alone will kill 80% of wasted cycles.

🧠 2. Normalize + validate actions BEFORE execution
You’re rejecting bad done after the fact — too late.

Fix:
Create a validator layer:

fn validate_action(action: &Action) -> Result<()> {
    match action.kind {
        "done" => {
            ensure!(action.reason.is_some(), "missing reason");
            ensure!(action.rationale.is_some(), "missing rationale");
        }
        _ => {}
    }
    Ok(())
}
Run this immediately after parsing — not inside execution.

🧩 3. Auto-repair invalid actions (huge UX win)
Instead of rejecting:

❌ “missing rationale”

👉 Fix it automatically:

if action.kind == "done" && action.rationale.is_none() {
    action.rationale = Some("Auto-filled rationale to satisfy schema".into());
}
This turns the system from strict → cooperative.

⛔ 4. Collapse noop actions at the executor level
You’re letting the agent spam:

{"action":"run_command","cmd":"echo \"idle\""}
Fix:
Detect and short-circuit:

if cmd.trim() == "echo \"idle\"" {
    return ContinueWithoutStep;
}
Or even better: don’t count it as a step.

🧭 5. Introduce “step intent” tracking
Right now every step is blind.

Add:

enum StepIntent {
    Explore,
    Modify,
    Verify,
    Complete,
}
Attach it to each step → enforce flow:

Explore → Modify → Verify → Done

NOT: idle → idle → idle → done

🧱 6. Make done a state, not just an action
Instead of trusting the agent:

if action.kind == "done" {
    if !has_meaningful_work_been_done() {
        reject("done too early");
    }
}
Track:

files read

patches applied

commands executed

🔄 7. Add retry with mutation (self-healing loop)
When something fails:

for attempt in 0..3 {
    let action = agent.next();

    if validate(action).is_ok() {
        break;
    }

    agent = mutate_prompt(agent, "Your last action was invalid because...");
}
This removes manual babysitting.

🧪 8. Separate “planning” vs “execution” loops
Right now everything is mixed → chaos.

Split:

Planner agent → expands tasks

Executor agent → performs actions

Verifier agent → checks correctness

You already almost have this — just enforce boundaries.

📉 9. Reduce strict JSON fragility
Your system is brittle because it expects perfection.

Add:

tolerant parser (allow trailing commas, missing fields)

fallback defaults

structured error messages fed back into loop

⚡ 10. Add a “fast-finish” path
If the agent clearly outputs a valid done, don’t loop again.

if action.kind == "done" && validate(&action).is_ok() {
    return Ok(action.reason);
}
💡 If you only implement 3 things:
Do these first:

No-progress watchdog

Auto-repair invalid actions

Pre-validation layer

That combination alone will make the system feel dramatically smarter and less frustrating.

If you want next step, I can:
👉 rewrite your run_agent_loop into a cleaner architecture
👉 or design a minimal “v2 executor” that removes most of this friction entirely


Show less
