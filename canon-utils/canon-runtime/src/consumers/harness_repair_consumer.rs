use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, PromptLoaded, RequestDispatch, RuntimeEvent};
use canon_loop::{HarnessRepairTarget, LoopStageExecutor};
use canon_proc_macros::must_emit;
use serde_json::json;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct HarnessRepairConsumer {
    workspace: PathBuf,
    tlog_path: PathBuf,
}

impl HarnessRepairConsumer {
    pub fn new(workspace: PathBuf, tlog_path: PathBuf) -> Self {
        Self {
            workspace,
            tlog_path,
        }
    }

    fn stamp_path() -> PathBuf {
        PathBuf::from("/workspace/ai_sandbox/canon/state/harness_repair_driver.json")
    }

    fn load_input() -> Option<(HarnessRepairTarget, String)> {
        let crate_name = env::var("CANON_HARNESS_REPAIR_CRATE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let failing_test = env::var("CANON_HARNESS_REPAIR_TEST")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let stderr = env::var("CANON_HARNESS_REPAIR_STDERR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                env::var("CANON_HARNESS_REPAIR_STDERR_FILE")
                    .ok()
                    .and_then(|path| fs::read_to_string(path).ok())
                    .filter(|value| !value.trim().is_empty())
            })?;
        Some((HarnessRepairTarget::new(crate_name, failing_test), stderr))
    }

    fn fingerprint(target: &HarnessRepairTarget, stderr: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        target.crate_name.hash(&mut hasher);
        target.failing_test.hash(&mut hasher);
        stderr.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn persist_fingerprint(fingerprint: &str) {
        let stamp_path = Self::stamp_path();
        if let Some(parent) = stamp_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(
            &stamp_path,
            serde_json::to_string(&json!({
                "fingerprint": fingerprint,
            }))
            .unwrap_or_else(|_| "{}".to_string()),
        );
    }

    fn prompt(target: &HarnessRepairTarget, directive: &canon_loop::HarnessRepairDirective, stderr: &str) -> String {
        let verifier = directive
            .verifier_command
            .as_deref()
            .unwrap_or("cargo check");
        format!(
            "Harness self-repair target:\n- crate: {}\n- failing test: {}\n\nFailure output:\n{}\n\nExecute exactly one constrained repair step.\n- next phase: {:?}\n- next action: {:?}\n- reason: {}\n- required verifier after mutation: {}\n\nDo not emit multiple mutating actions. If no actionable failure is scoped, refresh diagnostics instead of repairing.",
            target.crate_name.as_deref().unwrap_or("unknown"),
            target.failing_test.as_deref().unwrap_or("unknown"),
            stderr.trim(),
            directive.decision.phase,
            directive.decision.action,
            directive.decision.reason,
            verifier,
        )
    }

    fn should_trigger(prompt: &PromptLoaded) -> bool {
        prompt
            .payload
            .get("path")
            .and_then(|v| v.as_str())
            .map(Path::new)
            .and_then(|path| path.file_name().and_then(|v| v.to_str()))
            == Some("AGENT_GOAL.md")
    }
}

impl EventConsumer for HarnessRepairConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "harness_repair_consumer" }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        let RuntimeEvent::PromptLoaded(prompt) = event else {
            return EventOutcome::NoOp("harness_repair_consumer_non_prompt");
        };
        if !Self::should_trigger(prompt) {
            return EventOutcome::NoOp("harness_repair_consumer_irrelevant_prompt");
        }
        let Some((target, stderr)) = Self::load_input() else {
            return EventOutcome::NoOp("harness_repair_consumer_no_input");
        };
        let fingerprint = Self::fingerprint(&target, &stderr);

        let mut executor = LoopStageExecutor::new(self.workspace.clone(), self.tlog_path.clone());
        let directive = executor.evaluate_harness_repair_for_target(&target, &stderr);
        let prompt = Self::prompt(&target, &directive, &stderr);

        Self::persist_fingerprint(&fingerprint);
        let _ = trigger_id;
        let prompt = format!(
            "{}\n\nHarness repair directive:\n{}",
            prompt,
            serde_json::to_string(&json!({
                "target": {
                    "crate_name": target.crate_name,
                    "failing_test": target.failing_test,
                },
                "decision": {
                    "phase": format!("{:?}", directive.decision.phase),
                    "action": format!("{:?}", directive.decision.action),
                    "reason": directive.decision.reason,
                },
                "verifier_command": directive.verifier_command,
                "fingerprint": fingerprint,
            }))
            .unwrap_or_default()
        );
        EventOutcome::emit(
            RuntimeEvent::RequestDispatch(RequestDispatch {
                dispatch_id: Uuid::new_v4().to_string(),
                parent_request_id: "harness_repair_driver".to_string(),
                agent_id: "exec".to_string(),
                task_prompt: prompt,
                task_kind: "harness_repair".to_string(),
                deps: Vec::new(),
                workspace_scope: Some(self.workspace.display().to_string()),
                dispatched: true,
            }),
            file!(),
            line!(),
        )
    }
}
