# Canon Event Semantic Rules

1. `LoopObserved` sets `context_ready`. Nothing routes or plans until `context_ready=true`.
2. Every `LoopPlanned` (non-`no_op`) must have exactly one corresponding `LoopActed`.
3. Every `ToolCall` must have exactly one corresponding `ToolResult`. `ToolBatchSettled` fires only when all open `ToolCall`s are closed and `planned_pending=0`.
4. `LoopVerified` sets `finish_ready`. When `finish_ready=true`, the router must select `conclude`.
5. `LoopRewarded { halt: true }` is terminal — no stage executes after it.
6. `RouteSelected` is the only event that triggers stage execution. Stages never self-trigger.
7. `pending_request_id` is a mutex: only one router LLM call in-flight at a time. It clears on `CapabilityCompleted` or `CapabilityFailed` matching that request.

## Canonical `.log` — Allowed Entries

8. Only registered `CanonPayload` variants may be written.
9. Each entry is one complete JSON object per line, append-only. Entries are never modified or deleted after writing.
10. Every entry must carry `meta.ts` (unix ms), `meta.source` (emitting component). `event_id` is assigned by the segment writer — callers set it to `None`.
11. A new event kind requires a new variant in `CanonPayload` (`wire.rs`) and a re-export in `lib.rs`. Raw JSON must not be injected under an unregistered kind.
