use canon_analysis::ReportEventConsumer;
use canon_analysis::CapabilityEventConsumer;
use canon_analysis::SmtConsumer;
use canon_editor::EditConsumer;
use canon_graph::GraphConsumer;
use canon_query::QueryConsumer;
pub use canon_types::*;

struct KernelConsumerAdapter {
    inner: Box<dyn canon_types::KernelEventConsumer>,
}

impl KernelConsumerAdapter {
    fn new(inner: Box<dyn canon_types::KernelEventConsumer>) -> Self {
        Self { inner }
    }
}

impl canon_types::RuntimeConsumer for KernelConsumerAdapter {
    fn filter(&self) -> canon_types::RuntimeEventFilter {
        RuntimeEventFilter::Kernel(self.inner.mask())
    }

    fn on_event(&mut self, event: &canon_types::RuntimeEvent) {
        let canon_types::RuntimeEvent::Kernel { delta, state } = event else {
            return;
        };
        self.inner.on_event(delta, state);
    }
}

pub fn build_consumers() -> Vec<Box<dyn canon_types::RuntimeConsumer>> {
    vec![
        Box::new(KernelConsumerAdapter::new(Box::new(GraphConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(QueryConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(SmtConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(ReportEventConsumer::new()))),
        Box::new(KernelConsumerAdapter::new(Box::new(CapabilityEventConsumer::new()))),
        Box::new(EditConsumer::new()),
    ]
}
