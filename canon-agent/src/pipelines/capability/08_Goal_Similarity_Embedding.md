# Implementation Plan — 08 Goal Similarity Embedding

## Variables

g = current goal  
gₜ = template goal  
e(g) = embedding vector of goal  
S(g,gₜ) = similarity score  
T = template set  

---

## Equations

Embedding

e(g) ∈ ℝⁿ

Similarity

S(g,gₜ) = cosine(e(g), e(gₜ))

Template selection

T* = argmax S(g,gₜ)

Combined score

score = α·similarity + β·template_reward

---

# Objective

Improve template reuse by selecting templates based on **semantic similarity of goals**, not just structural graph similarity.

Current system:

TemplateIndex.find_similar()  
uses structural features.

New system:

Goal → Embedding → Similar template goals → Candidate templates.

---

# Architecture

Current

Goal → Planner → Graph

New

Goal  
↓  
Goal Embedding  
↓  
Template Search  
↓  
Reuse Template OR Run Planner

---

# Implementation Steps

## 1 Create Goal Embedding Module

New file

```

goal_embedding.rs

```

Structure

```

struct GoalEmbedding {
vector: Vec<f32>
}

```

Function

```

fn embed_goal(goal: &str) -> GoalEmbedding

```

Possible implementations

- local sentence embedding model
- LLM embedding endpoint

---

## 2 Store Template Goal Embeddings

Extend TemplateEntry

```

struct TemplateEntry {
goal
goal_embedding
}

```

Embedding stored when template is saved.

Location

templates.rs  
template_index.rs

---

## 3 Similarity Computation

Function

```

fn goal_similarity(a: &[f32], b: &[f32]) -> f64

```

Compute

```

cosine_similarity

```

Return value in

```

[0,1]

```

---

## 4 Template Retrieval

Modify

```

TemplateIndex::find_similar()

```

Combine scores

```

score = α·goal_similarity + β·structural_similarity

```

Return top_k templates.

---

## 5 Scheduler Integration

Location

scheduler.rs

Before planner invocation

```

candidates = template_index.find_similar(goal)

```

Then policy decides

```

reuse_template or run_planner

```

---

## 6 Embedding Cache

Avoid recomputing embeddings.

Cache file

```

agent_logs/goal_embeddings.json

```

Structure

```

goal_hash → embedding

```

---

## 7 Telemetry

Add metrics

```

goal_similarity_score
template_reuse_by_embedding
embedding_cache_hits

```

Stored in TelemetrySnapshot.

---

## 8 Config

Add parameters

```

embedding_model
embedding_dim
goal_similarity_weight
structural_similarity_weight

```

Allows tuning of retrieval.

---

# Files Modified

templates.rs  
template_index.rs  
scheduler.rs  
telemetry.rs  
config.rs  

New file

goal_embedding.rs

---

# Expected Impact

Template reuse improves across **semantically similar tasks**.

Examples

Goal

```

refactor Rust workspace

```

Matches template

```

clean Rust project

```

Even if structure differs.

---

# Result

System gains **semantic memory**.

Execution becomes

goal → similar template → reuse → faster execution
```

[
\max(I,E,C,A,R,P,S,D,T,K,X,B,L,F) = good
]
