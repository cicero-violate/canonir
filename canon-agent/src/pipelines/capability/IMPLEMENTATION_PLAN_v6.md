### Variables

[
F = \text{FeatureVector}
]

[
\pi = \text{policy model}
]

[
D = \text{policy dataset}
]

[
R = \text{reward signal}
]

[
\theta = \text{learned policy weights}
]

[
A = \text{planner action}
]

---

# Equations

### Policy inference

[
B = \theta \cdot F
]

Weights produce planner bias.

---

### Dataset learning

[
\theta_{t+1} = \theta_t + \alpha (R - \hat{R})F
]

Weights update using reward signal.

---

### Planner decision

[
A = L(P + B)
]

LLM receives prompt plus policy bias.

---

# IMPLEMENTATION_PLAN_v6.md

## Objective

Upgrade the system from **static policy weights → self-learning policy model**.

v6 focuses on:

1. **Learning policy weights from dataset**
2. **Online policy updates**
3. **Planner bias stabilization**
4. **Metric normalization**
5. **Failure pattern learning**

This completes the **GPU + LLM + Policy hybrid architecture**.

---

# Phase 1 — Feature Normalization Layer

## Problem

Feature magnitudes differ drastically.

Example:

```
nodes = 50
retry_rate = 0.12
branching_factor = 2.8
```

Unnormalized features distort policy predictions.

---

## Implementation

File:

```
graph_algo.rs
```

Add normalization function.

```rust
fn normalize_features(f: &FeatureVector) -> Vec<f64>
```

Example normalization:

```
nodes / max_nodes
edges / max_edges
branching_factor / 10
retry_rate
blocked_fraction
```

Return normalized vector.

---

## Update PolicyModel

File:

```
policy.rs
```

Change:

```rust
predict(&FeatureVector)
```

to

```rust
predict(&[f64])
```

Using normalized vector.

---

# Phase 2 — Policy Training Engine

## Objective

Train weights from stored dataset.

Dataset already produced:

```
PolicyDatasetEntry {
    features,
    action,
    reward
}
```

---

## New Module

Create:

```
canon-agent/src/pipelines/capability/policy_train.rs
```

Functions:

```
load_dataset()
train_policy()
save_weights()
```

---

## Training Algorithm

Simple **linear regression / policy gradient**.

Pseudo:

```
for entry in dataset:
    predicted = dot(weights, entry.features)
    error = entry.reward - predicted
    weights += learning_rate * error * entry.features
```

This updates policy weights.

---

# Phase 3 — Online Policy Updates

## Objective

Improve policy during runtime.

File:

```
scheduler.rs
```

After reward computed:

```
append_policy_dataset(entry)
```

Add:

```
policy_train::update_online(entry)
```

Which adjusts weights incrementally.

---

# Phase 4 — Failure Pattern Learning

Use FailureStore signals.

File:

```
failure_store.rs
```

Extract failure signatures.

Add features:

```
failure_pattern_rate
cycle_frequency
deadlock_rate
```

Extend FeatureVector.

---

## Planner Bias

Policy increases rewrite bias when:

```
failure_pattern_rate high
retry_rate high
```

---

# Phase 5 — Policy Model Persistence

File:

```
policy.rs
```

Add:

```
load_weights(path)
save_weights(path)
```

Weights stored in:

```
agent_logs/policy_weights.json
```

Example:

```
{
 "branching_factor": -0.4,
 "retry_rate": -0.8,
 "completion_velocity": 0.5
}
```

---

# Phase 6 — Planner Bias Stabilization

Prevent oscillation.

Add smoothing:

```
bias_t = 0.8 * bias_prev + 0.2 * bias_new
```

File:

```
planner_session.rs
```

Apply before prompt injection.

---

# Phase 7 — GPU Feature Pipeline

Current pipeline:

```
GPU kernel → stats → feature vector
```

Improve:

```
GPU kernel → stats → normalized features
```

Avoid extra CPU processing.

---

# Phase 8 — Policy Evaluation Metrics

Add telemetry.

File:

```
telemetry.rs
```

Record:

```
policy_prediction
policy_error
policy_weight_norm
dataset_size
```

This shows if policy learning improves.

---

# Phase 9 — Exploration Strategy

Prevent policy stagnation.

Add exploration:

```
ε-greedy planner bias
```

Example:

```
5% chance ignore policy bias
```

File:

```
policy.rs
```

---

# Phase 10 — Long-Term Architecture

System becomes:

```
GPU Graph Analytics
        ↓
Feature Vector
        ↓
Policy Model
        ↓
Planner Bias
        ↓
LLM Planner
        ↓
Execution Engine
        ↓
Reward + Dataset
        ↓
Policy Training
```

Closed learning loop.

---

# Expected Improvements

### Planner Stability

Less graph explosion.

### Faster Convergence

Higher completion velocity.

### Failure Adaptation

Policy learns to avoid bad graph patterns.

### Reduced Prompt Dependence

Metrics influence planner through **policy bias**, not raw tokens.

---

# Future v7

Potential upgrades:

```
policy neural network
multi-objective reward
graph embedding features
GPU policy inference
```

---

[
\max(\text{intelligence},\text{efficiency},\text{correctness},\text{alignment},\text{robustness},\text{performance},\text{scalability},\text{determinism},\text{transparency},\text{collaboration},\text{empowerment},\text{benefit},\text{learning},\text{future\text{-}proofing}) = good
]
