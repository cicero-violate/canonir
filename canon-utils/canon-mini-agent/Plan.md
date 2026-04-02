*** Begin Patch
*** Add File: PLANS/n-lane-executor-scaling.md
+# N-Lane Executor Scaling Plan
+
+## Objective
+
+Replace the hardcoded 2-lane executor architecture with a runtime N-lane system
+driven entirely by the number of `role = "executor"` endpoints in
+`capability_config.toml`. Adding a third (or fourth) executor requires only a
+new TOML endpoint block — no code changes.
+
+## Invariants That Must Hold After This Change
+
+- One `TabManagerHandle` per lane (already true, must stay true).
+- `ws_server::ServerState::pending` keyed by `(tab_id, turn_id)` so concurrent
+  lanes on different physical tabs never clobber each other's waiters.
+- `pending_send_ack` and `pending_submit` also keyed by `(tab_id, turn_id)`.
+- `TAB_CLOSED` drains all `(tab_id, *)` entries from every pending map.
+- Lane plan file path derived as `PLANS/executor-{endpoint_id}.md`.
+- Single shared verifier, planner, diagnostics endpoint (role strings unchanged).
+
+## File 1 — `canon-llm/src/ws_server.rs`
+
+### Change 1a — `pending` key type
+
+```rust
+// before
+pending: HashMap<u32, oneshot::Sender<String>>,
+
+// after
+pending: HashMap<(u32, u64), oneshot::Sender<String>>,
+```
+
+### Change 1b — `pending_send_ack` key type
+
+```rust
+// before
+pending_send_ack: HashMap<u32, oneshot::Sender<()>>,
+
+// after
+pending_send_ack: HashMap<(u32, u64), oneshot::Sender<()>>,
+```
+
+### Change 1c — `pending_submit` key type
+
+```rust
+// before
+pending_submit: HashMap<u32, oneshot::Sender<()>>,
+
+// after
+pending_submit: HashMap<(u32, u64), oneshot::Sender<()>>,
+```
+
+### Change 1d — `send_turn_with_meta` inserts
+
+All three insert/remove calls in `send_turn_with_meta` change from `tab_id` to
+`(tab_id, turn_id)`. The turn_id is already computed before the block. The
+ack-timeout and response-timeout cleanup paths also use `(tab_id, turn_id)`.
+
+### Change 1e — `submit_turn` inserts
+
+Same pattern as 1d for `pending_submit`.
+
+### Change 1f — `SUBMIT_ACK` handler
+
+```rust
+// before
+if let Some(tx) = st.pending_send_ack.remove(&tab_id) { ... }
+if let Some(tx) = st.pending_submit.remove(&tab_id) { ... }
+
+// after — ack_turn_id is already extracted from the frame
+if let Some(tx) = st.pending_send_ack.remove(&(tab_id, ack_turn_id)) { ... }
+if let Some(tx) = st.pending_submit.remove(&(tab_id, ack_turn_id)) { ... }
+```
+
+`ack_turn_id` must be present for the lookup to succeed. If it is absent,
+return early (already done for `pending_send_ack`; apply same guard to
+`pending_submit`).
+
+### Change 1g — `INBOUND_MESSAGE` resolution
+
+```rust
+// before
+if let Some(tx) = st.pending.remove(&tab_id) { tx.send(text) }
+
+// after
+if let Some(eid) = effective_turn_id {
+    if let Some(tx) = st.pending.remove(&(tab_id, eid)) {
+        let _ = tx.send(text);
+    } else {
+        st.completed_turns.push(assembled_record);
+    }
+} else {
+    st.completed_turns.push(assembled_record);
+}
+```
+
+### Change 1h — `TAB_CLOSED` drain
+
+```rust
+// before
+st.pending.remove(&tab_id);
+st.pending_send_ack.remove(&tab_id);
+st.pending_submit.remove(&tab_id);
+
+// after
+st.pending.retain(|(tid, _), _| *tid != tab_id);
+st.pending_send_ack.retain(|(tid, _), _| *tid != tab_id);
+st.pending_submit.retain(|(tid, _), _| *tid != tab_id);
+```
+
+## File 2 — `canon-mini-agent/src/main.rs`
+
+### Change 2a — Remove `LaneId` enum
+
+Delete the `LaneId` enum and all `LaneId::A` / `LaneId::B` references.
+Replace with `usize` lane index throughout.
+
+### Change 2b — `LaneConfig` struct (new, local to main.rs)
+
+```rust
+struct LaneConfig {
+    index: usize,
+    endpoint: LlmEndpoint,
+    plan_file: String,   // e.g. "PLANS/executor-exec_chatgpt_d.md"
+    label: String,       // e.g. "exec_chatgpt_d"
+    tabs: TabManagerHandle,
+}
+```
+
+Built at startup by filtering `config.llm_endpoints` for `role == "executor"`,
+sorted by `id` for deterministic ordering:
+
+```rust
+let lanes: Vec<LaneConfig> = config.llm_endpoints
+    .iter()
+    .filter(|e| e.role.as_deref() == Some("executor"))
+    .enumerate()
+    .map(|(i, ep)| LaneConfig {
+        index: i,
+        endpoint: ep.clone(),
+        plan_file: format!("PLANS/executor-{}.md", ep.id),
+        label: ep.id.clone(),
+        tabs: llm_worker_new_tabs(),
+    })
+    .collect();
+```
+
+Fail fast at startup if `lanes.is_empty()`.
+
+### Change 2c — `DispatchLaneState` key change
+
+```rust
+// before
+lanes: HashMap<LaneId, DispatchLaneState>
+
+// after
+lanes: HashMap<usize, DispatchLaneState>
+```
+
+Initialised by iterating `lanes.iter()` and inserting one entry per index.
+
+### Change 2d — All `LaneId`-keyed maps become `usize`-keyed
+
+Affected fields in `DispatchState`:
+- `submitted_turns: HashMap<(u32, u64), SubmittedExecutorTurn>` — unchanged
+- `pending_submits: HashMap<usize, PendingSubmitState>`
+- `tab_id_to_lane: HashMap<u32, usize>`
+- `lane_active_tab: HashMap<usize, u32>`
+- `lane_next_submit_at_ms: HashMap<usize, u64>`
+- `lane_submit_in_flight: HashMap<usize, bool>`
+
+### Change 2e — `SubmittedExecutorTurn` lane field
+
+```rust
+// before
+lane: LaneId,
+
+// after
+lane: usize,
+```
+
+### Change 2f — `PendingExecutorSubmit` lane field
+
+```rust
+// before
+lane_id: LaneId,
+lane_plan_file: &'static str,
+
+// after
+lane_index: usize,
+lane_plan_file: String,
+label: String,
+```
+
+### Change 2g — Submit loop
+
+Replace the two hardcoded `exec_a_job` / `exec_b_job` blocks with a loop over
+`lanes`:
+
+```rust
+for lane in &lanes {
+    let in_flight = *dispatch_state.lane_submit_in_flight
+        .get(&lane.index).unwrap_or(&false);
+    let next_at = *dispatch_state.lane_next_submit_at_ms
+        .get(&lane.index).unwrap_or(&0);
+    if in_flight || next_at > now {
+        continue;
+    }
+    if let Some(job) = claim_executor_submit(&mut dispatch_state, lane) {
+        // spawn submit_executor_turn into submit_joinset
+    }
+}
+```
+
+### Change 2h — `claim_executor_submit` signature
+
+```rust
+// before
+fn claim_executor_submit(state: &mut DispatchState, executor_name: &'static str)
+    -> Option<PendingExecutorSubmit>
+
+// after
+fn claim_executor_submit(state: &mut DispatchState, lane: &LaneConfig)
+    -> Option<PendingExecutorSubmit>
+```
+
+Looks up `state.lanes.get_mut(&lane.index)`, checks `pending &&
+in_progress_by.is_none()`, sets `in_progress_by = Some(lane.label.clone())`,
+returns `PendingExecutorSubmit` with `lane_index`, `lane_plan_file`, `label`
+filled from `LaneConfig`.
+
+### Change 2i — `submit_joinset` item type
+
+```rust
+// before
+JoinSet<(&'static str, PendingExecutorSubmit, Result<String>)>
+
+// after
+JoinSet<(usize, PendingExecutorSubmit, Result<String>)>
+```
+
+The `usize` is the lane index, replacing the executor name string used only
+for lane identification.
+
+### Change 2j — Verifier `lane_plan_file` lookup
+
+```rust
+// before
+let lane_plan_file = match submitted.lane {
+    LaneId::A => EXECUTOR_A_PLAN_FILE,
+    LaneId::B => EXECUTOR_B_PLAN_FILE,
+};
+
+// after
+let lane_plan_file = lanes[submitted.lane].plan_file.clone();
+```
+
+### Change 2k — Verifier summary vec
+
+```rust
+// before
+let mut verifier_summary = (String, String);
+
+// after
+let mut verifier_summary: Vec<String> =
+    vec!["(none yet)".to_string(); lanes.len()];
+```
+
+Planner prompt builds summary by joining:
+```rust
+let summary_text = lanes.iter()
+    .map(|l| format!("{}={}", l.label, verifier_summary[l.index]))
+    .collect::<Vec<_>>()
+    .join("\n");
+```
+
+### Change 2l — Remove static plan file constants
+
+`EXECUTOR_A_PLAN_FILE` and `EXECUTOR_B_PLAN_FILE` constants are deleted.
+All references replaced by `lane.plan_file` or `lanes[idx].plan_file`.
+
+### Change 2m — Single-role mode executor path
+
+The `--start executor` single-role path currently reads `exec_a_plan_path`.
+After this change it reads the plan file of `lanes[0]` (first executor
+endpoint). If no executor endpoints exist the binary exits with a clear error.
+
+### Change 2n — `dispatch_lane_mut` helper
+
+Signature stays the same structurally but takes `usize` instead of `LaneId`:
+
+```rust
+fn dispatch_lane_mut(state: &mut DispatchState, lane_index: usize)
+    -> &mut DispatchLaneState
+```
+
+## Sequencing
+
+1. Implement `ws_server.rs` changes (1a–1h) first — self-contained, no
+   dependency on main.rs shape.
+2. Implement `main.rs` changes (2a–2n) as a single commit since they are
+   heavily interdependent.
+3. `cargo check -p canon-mini-agent` after each file.
+4. Update `capability_config.toml` to rename existing executor endpoint roles
+   to `role = "executor"` and verify the two-lane case still works before
+   adding a third.
+
+## What Does Not Change
+
+- `tab_management.rs` — no changes needed.
+- `endpoint_worker.rs` — no changes needed.
+- `config.rs` — no changes needed. `LlmEndpoint.role` already exists.
+- The verifier, planner, diagnostics role strings and endpoint lookups.
+- `continue_executor_completion` — signature unchanged, `lane: usize` threads
+  through naturally.
+- The `JoinSet` / `VecDeque` pipeline structure — unchanged, just operates
+  over dynamic lane count.
*** End Patch