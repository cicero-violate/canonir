# Scheduling Repair Plan

## Objective

Eliminate control-plane starvation under bursty event load while preserving deterministic single-writer semantics.

## Confirmed Failure Mode

- Current architecture multiplexes control and event traffic into one queue with one consumer.
- Under heavy event bursts, control ticks are delayed, route loop stalls, and execution cadence collapses.
- `try_send` drop behavior in consumer fanout can lose control-critical transitions.

## Design Principles

1. Split planes:
- `Q_c` for control signals (`tick`, routing cadence).
- `Q_e` for event stream (`event`, `reset`).

2. Interleaved scheduling (not strict priority):
- Always process up to one control step per cycle.
- Then process up to `N` event messages (`N` bounded).
- Prevents starvation in both directions.

3. Reliable control delivery:
- Control-critical events must not be dropped.
- Event-plane traffic may remain best-effort under pressure.

4. Keep single-writer commit:
- Only runtime main loop mutates runtime state and appends to tlog.

## Runtime Loop Contract

Per scheduler cycle:

1. Process one control message (if available).
2. Process at most `event_budget_per_cycle` event messages.
3. If no work was performed, block waiting for control/event input.

## Event Budget

- Configurable by `CANON_EVENT_RUNTIME_EVENT_BUDGET`.
- Default: `256`.
- Guarantee: finite upper bound on event work between control opportunities.

## Delivery Policy

- Control-critical bus events: blocking `send` (no drop).
- Non-control events: `try_send` (best-effort).

## Acceptance Criteria

1. Tick-to-route latency remains bounded during high event throughput.
2. No control-loop stalls when segmented `.log` files rotate.
3. No dropped control transitions (`Loop*`, capability lifecycle, prompt updates).
4. Event throughput remains steady without starving control.
5. Single-writer determinism is preserved.

## Rollout Notes

- Observe `route_selected`, `loop_observed`, `loop_planned`, `loop_acted`, `loop_verified`, `loop_rewarded` continuity across segment boundaries.
- Tune `CANON_EVENT_RUNTIME_EVENT_BUDGET` if control jitter or event lag appears.

