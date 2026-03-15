use canon_planner::CapabilityEventConsumer;
use canon_planner::SmtConsumer;
use canon_planner::GraphConsumer;
use canon_editor::EditConsumer;
use canon_event::{KernelEventConsumer, RuntimeConsumer, RuntimeEvent, RuntimeEventFilter};
use canon_query::QueryConsumer;

struct KernelConsumerAdapter {
    inner: Box<dyn KernelEventConsumer>,
}

impl KernelConsumerAdapter {
    fn new(inner: Box<dyn KernelEventConsumer>) -> Self {
        Self { inner }
    }
}

impl RuntimeConsumer for KernelConsumerAdapter {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::Kernel(self.inner.mask())
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::Kernel { delta, state } = event else {
            return;
        };
        self.inner.on_event(delta, state);
    }
}

pub fn build_consumers() -> Vec<Box<dyn RuntimeConsumer>> {
    vec![
        Box::new(KernelConsumerAdapter::new(Box::new(GraphConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(QueryConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(SmtConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(CapabilityEventConsumer::new()))),
        Box::new(EditConsumer::new()),
    ]
}
