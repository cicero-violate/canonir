use canon_event::{new_error_occurred, EventConsumer, EventFilter, EventId, EventOutcome, RuntimeEvent};
use canon_proc_macros::must_emit;
use std::collections::HashMap;

const STAGE_THRESHOLDS: &[(&str, u64)] = &[("observed", 10), ("planned", 15), ("acted", 15), ("verified", 20), ("rewarded", 25)];

pub struct WatchdogConsumer {
    current_tick: u64,
    last_stage_tick: HashMap<&'static str, u64>,
}

impl WatchdogConsumer {
    pub fn new() -> Self {
        let mut last_stage_tick = HashMap::new();
        for (stage, _) in STAGE_THRESHOLDS {
            last_stage_tick.insert(*stage, 0u64);
        }
        Self { current_tick: 0, last_stage_tick }
    }
}

impl EventConsumer for WatchdogConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "watchdog_consumer" }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        match event {
            RuntimeEvent::Tick(t) => {
                self.current_tick = t.tick;
                let stalled: Vec<RuntimeEvent> = STAGE_THRESHOLDS
                    .iter()
                    .filter_map(|(stage, threshold)| {
                        let last = self.last_stage_tick.get(stage).copied().unwrap_or(0);
                        let idle = self.current_tick.saturating_sub(last);
                        // Fire at the threshold, then once every threshold ticks after that.
                        // Avoids emitting a warning every tick (which floods LoopObserved via
                        // trigger_observe in LoopStageExecutor).
                        if idle > 0 && idle % threshold == 0 {
                            Some(RuntimeEvent::ErrorOccurred(new_error_occurred(
                                "watchdog_stall",
                                "watchdog",
                                format!("Stage '{stage}' has not fired in {idle} ticks"),
                                "warning",
                                serde_json::json!({ "stage": stage, "idle_ticks": idle }),
                                None,
                            )))
                        } else {
                            None
                        }
                    })
                    .collect();
                if stalled.is_empty() {
                    EventOutcome::NoOp("watchdog_all_stages_healthy")
                } else {
                    EventOutcome::emit_many(stalled, file!(), line!())
                }
            }
            RuntimeEvent::LoopObserved(_) => {
                self.last_stage_tick.insert("observed", self.current_tick);
                EventOutcome::NoOp("watchdog_stage_reset")
            }
            RuntimeEvent::PlanningCompleted(_) => {
                self.last_stage_tick.insert("planned", self.current_tick);
                EventOutcome::NoOp("watchdog_stage_reset")
            }
            RuntimeEvent::LoopActed(_) => {
                self.last_stage_tick.insert("acted", self.current_tick);
                EventOutcome::NoOp("watchdog_stage_reset")
            }
            RuntimeEvent::LoopVerified(_) => {
                self.last_stage_tick.insert("verified", self.current_tick);
                EventOutcome::NoOp("watchdog_stage_reset")
            }
            RuntimeEvent::LoopRewarded(_) => {
                self.last_stage_tick.insert("rewarded", self.current_tick);
                EventOutcome::NoOp("watchdog_stage_reset")
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::RequestDispatch(_)
            | RuntimeEvent::SubTaskResult(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::AgentRegistered(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_) => EventOutcome::NoOp("watchdog_not_a_stage_event"),
        }
    }
}
