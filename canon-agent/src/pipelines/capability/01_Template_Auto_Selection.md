# Implementation Plan — Template Auto-Selection

## Variables

N = number of stored templates  
G = current task graph  
T = template graph  
S(g,t) = similarity score between goal g and template t  
R(t) = stored reward for template t  
P = policy decision  

---

## Equations

Template selection score

S_t = α · similarity(goal, template_goal) + β · reward(t)

Select template

T* = argmax(S_t)

Planner decision

P = policy(features)

Execution decision

if P.run_planner = false → load template  
else → run planner

---

## Objective

Allow the agent to **reuse high-quality DAG templates automatically instead of invoking the planner**.

Current system already has:

- TemplateStore
- TemplateIndex
- Template similarity search
- Policy decision system

Missing piece is **wiring the decision into the planner loop**.

---

# Architecture

Current loop

Goal → Planner → Graph → Scheduler → Executor

New loop

Goal → Template Search → Policy Decision  
  → (Reuse Template) OR (Run Planner)

Graph → Scheduler → Executor → Reward → Template Update

---

# Implementation Steps

## 1. Add Template Selection Stage

Location:

scheduler.rs  
run_planner_execution_loop()

Before planner invocation:

```

let candidates = store.find_similar(goal, graph, 5);

```

Compute score

```

score = similarity * reward

```

Choose best candidate.

---

## 2. Policy Decision Gate

Policy already outputs:

```

PolicyDecision {
run_planner,
expansion_scale,
prioritize_unblock,
execution_preference
}

```

Add template decision:

```

reuse_template = !run_planner

```

Logic

```

if reuse_template && candidate_score > threshold
load_template()
else
run_planner()

```

---

## 3. Template Load Path

TemplateStore already supports:

```

TemplateStore::load(name)

```

Add code:

```

graph = store.load(template_name)
graph.reset_for_execution()

```

---

## 4. Template Reward Update

At run completion:

```

reward = telemetry::compute_reward(...)
store.save_with_reward(template_name, graph, reward)

```

Already partially implemented.

Ensure reward is stored.

---

## 5. Failure Handling

If template execution fails:

```

store.record_failure(template_hash)

```

Increase failure count in TemplateIndex.

Templates with high failure rate become less likely to be reused.

---

## 6. Telemetry Additions

Add metrics:

template_reuse
template_score
template_selected

Example:

```

RuntimeMetrics {
template_reuse: bool
}

```

---

## 7. Config Options

Add config fields:

```

template_reuse_threshold
template_top_k

```

Used for template selection.

---

# Execution Flow

1. Goal received
2. Template search
3. Policy decision
4. Either:

Load template DAG  
or  
Run planner

5. Execute graph
6. Compute reward
7. Update template store
8. Continue loop

---

# Expected Impact

Planner calls reduced by

~70–90%

Execution becomes mostly:

Goal → Template → Execution

Instead of

Goal → Planner → Execution

---

# Files Modified

scheduler.rs  
templates.rs  
template_index.rs  
telemetry.rs  
config.rs

---

# Result

Autonomous system behavior improves:

- Faster execution
- Reduced planner load
- Reuse of successful workflows
- Continuous improvement

System approaches:

Goal → Autonomous Execution

