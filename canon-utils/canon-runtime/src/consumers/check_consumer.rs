use canon_check::{default_checks, run_checks, Check, CheckResult};
use canon_event::{RuntimeEvent, EventConsumer, EventEmitterHandle, EventFilter};

pub struct CheckConsumer {
    checks: Vec<Box<dyn Check>>,
    emitter: Option<EventEmitterHandle>,
}

impl CheckConsumer {
    pub fn new() -> Self {
        Self { checks: default_checks(), emitter: None }
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

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        // Only handle Debug events; build a minimal JSON view
        let RuntimeEvent::Debug(d) = event else {
            return;
        };
        if d.source == "canon_check" {
            return;
        }
        let value = serde_json::json!({ "source": d.source, "kind": d.kind, "payload": d.payload });

        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };

        for result in run_checks(&self.checks, &value) {
            let (severity, items): (&str, Vec<(&str, &str)>) = match &result {
                CheckResult::Warn(w) => ("warn", w.iter().map(|x| (x.check, x.message.as_str())).collect()),
                CheckResult::Err(e) => ("error", e.iter().map(|x| (x.check, x.message.as_str())).collect()),
                CheckResult::Ok => continue,
            };
            for (check, message) in items {
                let payload = serde_json::json!({
                    "check": check,
                    "severity": severity,
                    "message": message,
                });
                let _ = canon_meta::canon_emit_meta!(emitter; "canon_check", severity, payload);
            }
        }
    }
}
