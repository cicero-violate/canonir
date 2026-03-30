extern crate canon_event;

mod aggregator;
mod consumer;
mod metrics;
mod reducer;
pub mod reducers;
mod storage;

pub use aggregator::{compute_g, compute_reward, normalize};
pub use consumer::GoodnessConsumer;
pub use metrics::Metrics;
pub use reducer::Reducer;
pub use storage::MetricsStorage;
