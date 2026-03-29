use canon_check::{default_checks, run_checks, Check};
use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use canon_proc_macros::must_emit;

pub struct CheckConsumer {
    checks: Vec<Box<dyn Check>>,
}

impl CheckConsumer {
    pub fn new() -> Self {
        Self { checks: default_checks() }
    }
}

impl Default for CheckConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventConsumer for CheckConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "check_consumer"
    }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        let RuntimeEvent::Debug(d) = event else {
            return EventOutcome::NoOp("check_consumer_non_debug");
        };
        if d.source == "canon_check" {
            return EventOutcome::NoOp("check_consumer_ignore_self");
        }
        let value = serde_json::json!({ "source": d.source, "kind": d.kind, "payload": d.payload });
        let _ = run_checks(&self.checks, &value);
        EventOutcome::NoOp("check_consumer_ran")
    }
}
