You are designing an implementation plan for the MIR → Canon lowering layer.

STRICT REQUIREMENTS

1. Do NOT propose safety fallbacks.
2. Do NOT introduce placeholder expressions.
3. Do NOT suppress assignments or emit panic sentinels.
4. Do NOT weaken invariants.

The kernel invariant must remain strict:
every MIR construct must lower deterministically into Canon IR.

GOAL

Expand the lowering system so that ALL MIR rvalue variants are structurally supported.

This means the plan must:

• Enumerate the complete MIR Rvalue and Operand space.
• Identify which variants are currently unsupported.
• Provide deterministic lowering rules for each missing variant.
• Ensure lowering never returns None for valid MIR.

Specifically investigate:

- Rvalue::Cast variants
- CastKind::Transmute
- pointer casts
- constant operands from compiler intrinsics
- associated constants used in stdlib MIR
- BinaryOp with constant operands

PLAN REQUIREMENTS

The plan must include:

1. Full MIR coverage audit
2. Mapping table: MIR construct → Canon IR representation
3. Exact code locations to modify
4. Deterministic lowering rules
5. Invariant-preserving handling

PROHIBITED SOLUTIONS

Do not propose:

- panic fallbacks
- sentinel expressions
- suppression logic
- skipping assignments
- weakening invariant checks

The objective is to make the lowering layer COMPLETE, not tolerant.

Output the plan as a structured implementation strategy.
