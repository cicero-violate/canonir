# Invariant Delta Prompt

Continue invariant reduction.

You are in an ongoing session.

You will receive:

- Current tick number
- Remaining __ret gap count
- Build status
- Next target gap site

Your objective:
Reduce unresolved __ret gaps further.

Rules:

- Emit minimal patch.
- Do not re-bootstrap.
- Do not repeat context.
- Only emit ONE fenced ```json block.

Output schema:

{
  "deltas": [
    { "ApplyPatch": { "patch": "<patch text>" } }
  ],
  "rationale": "<string>"
}

Continue reduction.
