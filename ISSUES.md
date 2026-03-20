# Issues (Snapshot: `state/reports_out/llm`, 2026-03-20 01:01)

## 1) Partial execution visibility for multi-block planner output
- Severity: High
- Evidence:
  - `0000_response.json` shows `parse_blocks=5` and `valid_action_count=5`.
  - `0000_tool_call.json` contains only 2 dispatched calls.
  - `0000_tool_results.json` contains 2 rows, with the second still `status="pending"`.
- Impact:
  - Artifact set appears incomplete even though planner returned 5 actions.
  - Reviewers cannot determine whether remaining actions are queued, dropped, or blocked without additional runtime/tlog inspection.
- Expected:
  - Either all actions from one planner response become visible in artifacts promptly, or artifacts include explicit queue/backlog metadata for undispatched steps.

## 2) Pending tool result remains open in artifact
- Severity: High
- Evidence:
  - `0000_tool_results.json` has a `pending` entry for:
    - `request_id=fc458c0c-0b32-42a8-a670-8d4a1c5bd48e`
    - `tool_call_id=32bbe5a3-c60e-4e50-8dcb-1c5a0ea2037a`
  - No terminal fields (`tool_result_id`, `finalized_ms`, terminal output) yet.
- Impact:
  - The run state is not terminally observable from artifacts at capture time.
  - External reviewers may treat this as a lost completion unless reconciled.
- Expected:
  - Pending should transition to terminal (`completed`/`failed`) quickly, or be reconciled to synthetic failure on timeout/restart.

## 3) Risky destructive command accepted without visible guardrail
- Severity: Medium
- Evidence:
  - `0000_response.json` first action is `rm -rf /workspace/ai_sandbox/canon/test_rust_project_v3`.
  - `0000_tool_call.json` confirms dispatch of that command.
- Impact:
  - Planner can emit destructive actions that execute immediately in this loop.
  - In broader goals this can cause unintended data loss.
- Expected:
  - Explicit policy gate for destructive commands (allowlist/confirmation/sandbox scope checks) before dispatch.

## 4) No explicit “batch complete” marker per planner response index
- Severity: Medium
- Evidence:
  - Artifacts exist per index (`0000_*`), but there is no completion sentinel indicating whether all planned actions for that index finished.
- Impact:
  - A snapshot can look broken when it is merely in-flight.
  - Makes automated health checks noisy.
- Expected:
  - Add batch lifecycle marker for each index (e.g., `status: in_progress|completed|failed_partial`, plus planned/executed counts).

---

## Non-issues verified in this snapshot
- Request message format is correct:
  - `0000_request.json` prompt starts with embedded JSON `{"request_id":"..."}`.
  - No `[REQ_ID:N]` prefix found.
- Parser diagnostics for planner response are correct:
  - `parse_ok=true`, `parse_mode=multi_block`, `parse_blocks=5`.
- Tool artifact index mapping is correct:
  - `0000_tool_call.json` and `0000_tool_results.json` align with `0000_request/response`.
