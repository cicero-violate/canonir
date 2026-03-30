Fix this
{"id":"6835da16-3bcb-4b8f-924b-da91d5f5ee02","parent_ids":["1a4d2dbb-d63c-4552-a307-0d39a8ffe91e"],"actor":"repair_control_consumer","kind":"debug","ts":1774903473950,"payload":{"input":{"kind":"repair_transition","source":"repair_control_consumer"},"output":{"payload":{"illegal_regions":1,"note":"act_stall_enter_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"}},"delta":{"payload":{"illegal_regions":1,"note":"act_stall_enter_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"}},"meta":{"file":"canon-utils/canon-runtime/src/consumers/repair_control_consumer.rs","line":440},"data":{"kind":"repair_transition","payload":{"illegal_regions":1,"note":"act_stall_enter_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"},"source":"repair_control_consumer"}},"prev_event_id":"70816307-4548-4c29-80af-fbb8ff7882d2"}
{"id":"75af5d8d-3be2-4a0a-a43d-5feb22c14f8f","parent_ids":["4ea4d43c-002a-42bb-ba6c-a066b69177f9"],"actor":"repair_control_consumer","kind":"debug","ts":1774903624158,"payload":{"input":{"kind":"repair_transition","source":"repair_control_consumer"},"output":{"payload":{"illegal_regions":3,"note":"act_stall_recover_to_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"}},"delta":{"payload":{"illegal_regions":3,"note":"act_stall_recover_to_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"}},"meta":{"file":"canon-utils/canon-runtime/src/consumers/repair_control_consumer.rs","line":440},"data":{"kind":"repair_transition","payload":{"illegal_regions":3,"note":"act_stall_recover_to_classifying","signal":"act_stall","tick":0,"to_phase":"classifying"},"source":"repair_control_consumer"}},"prev_event_id":"0e984e35-d56b-435c-9904-7a31114e1236"}
{"id":"44080e94-04d4-4abd-b73d-a5e00032433f","parent_ids":["7d7fd938-22ca-4f68-a56a-f086f469efb1"],"actor":"rustc","kind":"code","ts":1774903624154,"payload":{"input":{"id":0,"tick":0},"output":{"InvariantViolation":{"message":"noop_spam; parent=7d7fd938-22ca-4f68-a56a-f086f469efb1; kind=planning_completed; reasons=route_executor:route_policy_planned_to_act; count=1","recorded":true}},"delta":{"graph_version":0},"meta":{"file":"canon-utils/canon-runtime/src/bus.rs","line":234},"data":{"delta":{"event":{"InvariantViolation":{"message":"noop_spam; parent=7d7fd938-22ca-4f68-a56a-f086f469efb1; kind=planning_completed; reasons=route_executor:route_policy_planned_to_act; count=1","recorded":true}},"id":0,"tick":0},"state":{"graph_version":0,"invariant_hash":"","known_edges":[],"known_files":[],"known_symbols":{},"last_event_id":0,"phase":"","removed_edges":[],"removed_symbols":[],"tick":0}}},"prev_event_id":"b1d95595-22ae-4257-b272-9ea40dc31b34"}
{"id":"a01d65de-8caf-4dc5-87a5-42b7f06d9168","parent_ids":["44080e94-04d4-4abd-b73d-a5e00032433f"],"actor":"event-runtime","kind":"error_occurred","ts":1774903624156,"payload":{"input":{"kind":"invariant_violation","message":"noop_spam; parent=7d7fd938-22ca-4f68-a56a-f086f469efb1; kind=planning_completed; reasons=route_executor:route_policy_planned_to_act; count=1","source":"rustc"},"output":{"captured":true},"delta":{"captured":true},"meta":{"file":"canon-utils/canon-runtime/src/bus.rs","line":234},"data":{"captured":true,"context":{},"error_id":"cbbda60e-c4bc-4d9e-b405-b5538e63f2f3","kind":"invariant_violation","message":"noop_spam; parent=7d7fd938-22ca-4f68-a56a-f086f469efb1; kind=planning_completed; reasons=route_executor:route_policy_planned_to_act; count=1","severity":"error","source":"rustc","trace_id":null}},"prev_event_id":"5d3f510e-6961-42a6-90c5-0af5c1365b36"}
{"id":"e0e7854a-75db-42a8-bb9b-207f838f0599","parent_ids":["0e941ad6-d35f-4880-927d-311cb0ce289b"],"actor":"act_stage","kind":"debug","ts":1774903624156,"payload":{"input":{"kind":"act_suppressed","source":"act_stage"},"output":{"payload":{"context":{"active_batch_llm_request_id":null,"last_action_kind":"run_command","pending_act_present":false,"reason":"route_selected(act) but scheduler is empty","scheduler_len":0},"reason":"act dispatch returned noop"}},"delta":{"payload":{"context":{"active_batch_llm_request_id":null,"last_action_kind":"run_command","pending_act_present":false,"reason":"route_selected(act) but scheduler is empty","scheduler_len":0},"reason":"act dispatch returned noop"}},"meta":{"file":"canon-utils/canon-loop/src/stage/act.rs","line":243},"data":{"kind":"act_suppressed","payload":{"context":{"active_batch_llm_request_id":null,"last_action_kind":"run_command","pending_act_present":false,"reason":"route_selected(act) but scheduler is empty","scheduler_len":0},"reason":"act dispatch returned noop"},"source":"act_stage"}},"prev_event_id":"ae622141-4238-4bf0-8eba-44a1d4f23204"}
{"id":"4ea4d43c-002a-42bb-ba6c-a066b69177f9","parent_ids":["0e941ad6-d35f-4880-927d-311cb0ce289b"],"actor":"event-runtime","kind":"error_occurred","ts":1774903624157,"payload":{"input":{"kind":"act_stall","message":"route_selected(act) but scheduler is empty","source":"act_stage"},"output":{"captured":true},"delta":{"captured":true},"meta":{"file":"canon-utils/canon-loop/src/stage/act.rs","line":245},"data":{"captured":true,"context":{"active_batch_llm_request_id":null,"last_action_kind":"run_command","pending_act_present":false,"reason":"route_selected(act) but scheduler is empty","scheduler_len":0},"error_id":"dbb3b3ef-e95b-475e-a467-ed4fbb9ffbe0","kind":"act_stall","message":"route_selected(act) but scheduler is empty","severity":"warning","source":"act_stage","trace_id":null}},"prev_event_id":"a01d65de-8caf-4dc5-87a5-42b7f06d9168"}


- [ ] Diagnose act_stall → classifying illegal transition
  1. Open canon-utils/canon-runtime/src/consumers/repair_control_consumer.rs at line ~440
  2. Locate emission of repair_transition with signal="act_stall"
  3. Inspect current phase/state before transition is emitted
  4. Verify expected FSM successor for Act phase (should NOT be classifying)
  5. Add temporary log printing previous phase and decision inputs

- [ ] Prevent illegal Act → classifying transition
  1. Add guard in repair_control_consumer before emitting transition
  2. If current phase == Act and target == "classifying", block transition
  3. Redirect to valid successor (verify or observe based on state)
  4. Add debug_assert! to catch illegal transition during development
  5. Ensure no silent fallback to classifying remains

- [ ] Align repair transitions with routing invariants
  1. Confirm RouteSelected(act) leads to ToolCall/ToolResult path
  2. Ensure repair_transition does not override routing decision
  3. Add invariant check: Act phase cannot regress to earlier phase
  4. Log violation: "[INVARIANT] Act phase regression blocked"

- [ ] Validate via runtime logs
  1. Tail canon/state/log.txt
  2. Search for "act_stall" and "classifying"
  3. Confirm no Act → classifying transitions occur
  4. Verify correct sequence: Act → ToolCall → ToolResult → Verify
  5. Capture trace proving invariant is restored
