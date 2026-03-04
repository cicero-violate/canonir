#!/usr/bin/env python3
import json
from pathlib import Path

DATA_PATH = Path("/workspace/ai_sandbox/canon/agent_logs/policy_dataset.jsonl")
OUT_PATH = Path("/workspace/ai_sandbox/canon/agent_logs/policy_weights.json")

FEATURE_KEYS = [
    "nodes",
    "edges",
    "depth",
    "scc_count",
    "failure_rate",
    "reward_trend",
    "avg_out_degree",
    "avg_in_degree",
    "branching_factor",
    "leaf_count",
    "root_count",
    "verify_to_mutate_ratio",
    "observe_to_mutate_ratio",
    "node_type_entropy",
    "avg_node_priority",
    "avg_node_budget",
    "blocked_fraction",
    "ready_fraction",
    "failed_fraction",
    "completion_velocity",
    "retry_rate",
]

def load_rows():
    rows = []
    if not DATA_PATH.exists():
        return rows
    with DATA_PATH.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows

def compute_weights(rows):
    if not rows:
        return [0.0 for _ in FEATURE_KEYS]
    xs = []
    ys = []
    for row in rows:
        feats = row.get("features", {})
        x = [float(feats.get(k, 0.0)) for k in FEATURE_KEYS]
        y = float(row.get("reward", 0.0))
        xs.append(x)
        ys.append(y)
    # simple closed-form per-feature slope: sum(x*y)/sum(x*x)
    weights = []
    for i in range(len(FEATURE_KEYS)):
        num = sum(x[i] * y for x, y in zip(xs, ys))
        den = sum(x[i] * x[i] for x in xs) + 1e-6
        weights.append(num / den)
    return weights

def main():
    rows = load_rows()
    weights = compute_weights(rows)
    payload = {
        "planner_bias": weights,
        "node_add_bias": weights,
        "edge_add_bias": weights,
        "rewrite_bias": weights,
    }
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(payload, indent=2))
    print(f"wrote {OUT_PATH}")

if __name__ == "__main__":
    main()
