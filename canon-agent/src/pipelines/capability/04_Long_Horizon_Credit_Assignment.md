# Implementation Plan — 04 Long-Horizon Credit Assignment

## Variables

G = task graph  
π = policy  
aₜ = planner action at iteration t  
R = final run reward  
ΔG = planner graph update (nodes + edges + rewrites)  
D = policy dataset  

---

## Equations

Planner action credit

Credit(aₜ) = R − baseline

Baseline

baseline = avg(recent_rewards)

Policy update

π ← π + α · Credit(aₜ) · features

Dataset entry

D ← (features, action, policy_decision, reward)

---

# Objective

Assign **reward to planner decisions based on downstream execution success**.

Current system records reward only for execution outcome.

This implementation connects:

planner decisions → final run reward → policy learning

---

# Architecture

Current

Planner → Graph → Execute → Reward

New

Planner → Graph → Execute → Reward  
              ↓  
      Credit Assignment → Policy Training

---

# Planner Actions

Planner actions are already encoded in:

```

PlannerUpdate {
new_nodes,
new_edges,
retract_nodes,
rewrite_nodes
}

```

Each update becomes a **training action**.

---

# Implementation Steps

## 1 Capture Planner Action

Location

planner_session.rs

After planner iteration

```

let action = PlannerUpdate

```

Record planner decision.

---

## 2 Attach Feature Vector

Already available from

```

graph_features(graph)

```

Normalize

```

normalize_features(features)

```

These represent **state at decision time**.

---

## 3 Compute Reward

Existing implementation

```

telemetry::compute_reward(...)

```

Reward definition

```

R = progress_fraction − iteration_penalty

```

Higher reward = faster successful completion.

---

## 4 Credit Assignment

Compute planner credit

```

credit = reward − avg_recent_reward

```

Use TemplateStore history for baseline.

```

baseline = store.recent_rewards(template, k)

```

---

## 5 Dataset Entry

Append entry

```

PolicyDatasetEntry {
features,
action,
policy_decision,
reward
}

```

Write to

```

agent_logs/policy_dataset.jsonl

```

---

## 6 Online Policy Update

Location

policy_train.rs

Call

```

update_online(entry)

```

Policy weights updated using gradient step.

---

## 7 Planner Bias Update

Update planner bias using learned weights.

```

bias = policy.predict(features)

```

Apply bias

```

planner_bias
node_add_bias
edge_add_bias
rewrite_bias

```

Already supported in PolicyModel.

---

# Telemetry

Add metrics

```

planner_credit
reward_baseline
policy_update_norm

```

Stored in TelemetrySnapshot.

---

# Config

Add parameters

```

credit_baseline_window
policy_learning_rate

```

Controls stability of updates.

---

# Files Modified

planner_session.rs  
policy_train.rs  
scheduler.rs  
telemetry.rs  
config.rs  

---

# Expected Impact

Planner decisions become **reward-driven**.

Bad plans receive negative credit.

Good plans become more likely.

Learning loop becomes

planner → execution → reward → policy update

---

# Result

Planner improves over time.

System evolves toward

goal → learned planning → faster execution
```

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]
