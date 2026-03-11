use analysis_engine::SmtConsumer;
use canon_graph::GraphConsumer;
use canon_query::QueryConsumer;
use canon_reports::ReportConsumer;
pub use canon_types::*;

pub fn build_consumers() -> Vec<Box<dyn canon_types::KernelEventConsumer>> {
    vec![
        Box::new(GraphConsumer::new()),
        Box::new(QueryConsumer::new()),
        Box::new(SmtConsumer::new()),
        Box::new(ReportConsumer::new()),
    ]
}
