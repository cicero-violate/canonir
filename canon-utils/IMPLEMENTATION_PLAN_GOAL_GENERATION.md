● Now I have the full picture. Here's the implementation plan:

  ---
  Implementation Plan: LLM-Generated Goals at Bootstrap

  Overview

  Three coordinated changes:
  1. Clean /workspace/ai_sandbox/canon/test_projects on every bootstrap before the tlog is initialized
  2. Clear the existing AGENT_GOAL.md so the old goal doesn't prime the planner while waiting for the new one
  3. GoalGenConsumer — a new consumer that fires on the first tick, calls the planner LLM with a meta-prompt, and writes the
  generated goal as a PromptLoaded tlog event

  The existing pipeline then runs unchanged: observe detects the new PromptLoaded → emits LoopObserved with the new goal →
  planning begins.

  ---
  File changes

  1. canon-utils/canon-runtime/src/bin/event_runtime.rs

  Before bootstrap_config(...):
  clean_test_projects();
  clear_agent_goal();           // truncate AGENT_GOAL.md to placeholder

  Add to consumers vec (before LoopStageExecutor so it fires first on each tick):
  Box::new(GoalGenConsumer::new(tlog_path.clone())),

  Add two functions:
  const TEST_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects";
  const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";

  fn clean_test_projects() {
      let p = Path::new(TEST_PROJECTS_DIR);
      let _ = fs::remove_dir_all(p);
      let _ = fs::create_dir_all(p);
  }

  fn clear_agent_goal() {
      // Write a non-parseable placeholder so observe fires but reports goal_text=None
      // until GoalGenConsumer writes the real goal.
      let _ = fs::write(AGENT_GOAL_PATH, "# goal-pending\n");
  }

  ---
  2. canon-utils/canon-runtime/src/bootstrap.rs

  Expose a public helper so GoalGenConsumer can write a PromptLoaded event to the tlog without duplicating logic:

  pub fn write_prompt_loaded_to_tlog(tlog_path: &Path, content: &str) {
      let hash = content_hash(content);
      let payload = serde_json::json!({
          "prompt_id": "AGENT_GOAL",
          "path": GOAL_PROMPT_FILE,
          "hash": hash,
          "content": content,
      });
      write_boot_event(tlog_path, "prompt_loaded", payload);
  }

  ---
  3. canon-utils/canon-runtime/src/consumers/goal_gen_consumer.rs (new file)

  use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, RuntimeEvent, LlmCall, LlmResult};
  use std::path::PathBuf;

  const AGENT_GOAL_PATH: &str = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
  const TEST_PROJECTS_DIR: &str = "/workspace/ai_sandbox/canon/test_projects";

  const GOAL_GEN_PROMPT: &str = r#"
  You are a software engineering challenge generator for a multi-agent Rust coding system.

  Generate a SINGLE complex Rust project specification in EXACTLY the format shown below.
  Output ONLY the markdown — no preamble, no explanation, nothing else.

  Rules:
  - The project MUST be a Rust binary crate
  - Must require 800+ lines of real implementation across multiple modules
  - Must be self-contained — only crates.io dependencies, no workspace deps
  - Target path MUST be under /workspace/ai_sandbox/canon/test_projects/<slug>
  - `cargo check` passing is the sole success criterion
  - Choose a different project category each time (VM, parser, CLI tool, scheduler, graph lib, etc.)

  OUTPUT FORMAT (replace <...> placeholders):

  # <Project Title>

  <One paragraph describing what the project does and why it is interesting.>

  ## Target
  - Project path: `/workspace/ai_sandbox/canon/test_projects/<slug>`

  ## Requirements

  <numbered list of 8–12 specific, concrete implementation requirements>
  "#;

  enum State {
      Waiting,
      Pending(String),  // request_id
      Done,
  }

  pub struct GoalGenConsumer {
      tlog_path: PathBuf,
      emitter: Option<EventEmitterHandle>,
      state: State,
  }

  impl GoalGenConsumer {
      pub fn new(tlog_path: PathBuf) -> Self {
          Self { tlog_path, emitter: None, state: State::Waiting }
      }
  }

  impl EventConsumer for GoalGenConsumer {
      fn filter(&self) -> EventFilter { EventFilter::All }

      fn set_emitter(&mut self, emitter: EventEmitterHandle) {
          self.emitter = Some(emitter);
      }

      fn on_event(&mut self, event: &RuntimeEvent) {
          match (&self.state, event) {
              (State::Waiting, RuntimeEvent::Tick(_)) => {
                  let Some(emitter) = &self.emitter else { return };
                  let request_id = uuid::Uuid::new_v4().to_string();
                  emitter.emit(RuntimeEvent::LlmCall(LlmCall {
                      request_id: request_id.clone(),
                      prompt: GOAL_GEN_PROMPT.to_string(),
                      role: Some("planner".into()),
                      agent_id: None,
                  }));
                  self.state = State::Pending(request_id);
              }
              (State::Pending(expected_id), RuntimeEvent::LlmResult(result)) => {
                  if &result.request_id != expected_id { return; }
                  let content = extract_goal_text(&result.output);
                  if validate_goal(&content) {
                      let _ = std::fs::write(AGENT_GOAL_PATH, &content);
                      crate::bootstrap::write_prompt_loaded_to_tlog(&self.tlog_path, &content);
                      self.state = State::Done;
                  } else {
                      // Invalid output — retry on next tick
                      self.state = State::Waiting;
                  }
              }
              _ => {}
          }
      }
  }

  fn extract_goal_text(raw: &str) -> String {
      // Strip ```markdown ... ``` fence if the LLM wrapped it
      let trimmed = raw.trim();
      if let Some(inner) = trimmed.strip_prefix("```markdown").or_else(|| trimmed.strip_prefix("```")) {
          if let Some(inner) = inner.strip_suffix("```") {
              return inner.trim().to_string();
          }
      }
      trimmed.to_string()
  }

  fn validate_goal(content: &str) -> bool {
      // Must contain the required target path prefix and a Requirements section
      content.contains(TEST_PROJECTS_DIR) && content.contains("## Requirements")
  }

  ---
  4. canon-utils/canon-runtime/src/consumers/mod.rs

  Add:
  pub mod goal_gen_consumer;

  ---
  Sequence diagram

  bootstrap start
      clean_test_projects()          → /test_projects/ emptied
      clear_agent_goal()             → AGENT_GOAL.md = "# goal-pending\n"
      bootstrap_config()             → writes placeholder PromptLoaded to tlog
      GoalGenConsumer registered
      LoopStageExecutor registered

  tick 0
      GoalGenConsumer.on_event(Tick) → emits LlmCall(role=planner, prompt=GOAL_GEN_PROMPT)
      observe stage fires            → goal_text=None or "goal-pending" → LoopObserved (no real goal)
      route stage                    → no valid goal → Noop (needs verification in route executor)

  tick N  (LLM responds)
      LlmResult arrives
      GoalGenConsumer validates      → writes AGENT_GOAL.md + PromptLoaded to tlog
      observe stage detects change   → goal_hash changed → new LoopObserved with real goal
      route stage                    → RouteSelected → planning begins

  ---
  Key open question for codex

  The route executor needs to gracefully handle LoopObserved where goal_text is None or the placeholder. Verify that
  RouteExecutor already returns Noop (not a route) when goal is absent — if not, add a guard in canon-route/src/executor.rs.

  ---
  Dependency in Cargo.toml

  canon-runtime already depends on canon_event which has LlmCall/LlmResult. No new dependencies needed.
