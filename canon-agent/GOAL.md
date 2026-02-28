# GOAL

## Objective
Encode executable system mutation as a deterministic projection of structural IR evolution.

## Target Architecture

IR₀
  ↓ admit
Structural Delta
  ↓ apply
IR₁
  ↓ diff
CodeDelta
  ↓ project
apply_patch + bash

## Success Criteria
- CodeDelta generated deterministically
- No manual patch injection
- Shell emission derived solely from IR state transition
- Pipeline reward preserved

## Long-Term Direction
Unify:
  Structural correctness
  Execution authority
  Autonomous mutation
