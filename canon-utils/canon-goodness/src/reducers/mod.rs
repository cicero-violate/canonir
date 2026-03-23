mod intelligence;
mod efficiency;
mod correctness;
mod alignment;
mod robustness;
mod performance;
mod scalability;
mod determinism;
mod execution;
mod benefit;
mod learning;
mod future;
mod love;

use canon_event::RuntimeEvent;

use crate::Metrics;
use crate::Reducer;

pub struct AllReducers {
    pub i: intelligence::Intelligence,
    pub e: efficiency::Efficiency,
    pub c: correctness::Correctness,
    pub a: alignment::Alignment,
    pub r: robustness::Robustness,
    pub p: performance::Performance,
    pub s: scalability::Scalability,
    pub d: determinism::Determinism,
    pub x: execution::Execution,
    pub b: benefit::Benefit,
    pub l: learning::Learning,
    pub f: future::FutureProof,
    pub lambda: love::Love,
}

impl AllReducers {
    pub fn new() -> Self {
        Self {
            i: intelligence::Intelligence::default(),
            e: efficiency::Efficiency::default(),
            c: correctness::Correctness::default(),
            a: alignment::Alignment::default(),
            r: robustness::Robustness::default(),
            p: performance::Performance::default(),
            s: scalability::Scalability::default(),
            d: determinism::Determinism::default(),
        x: execution::Execution::default(),
            b: benefit::Benefit::default(),
            l: learning::Learning::default(),
            f: future::FutureProof::default(),
            lambda: love::Love::default(),
        }
    }

    pub fn update_all(&mut self, event: &RuntimeEvent) {
        self.i.update(event);
        self.e.update(event);
        self.c.update(event);
        self.a.update(event);
        self.r.update(event);
        self.p.update(event);
        self.s.update(event);
        self.d.update(event);
        self.x.update(event);
        self.b.update(event);
        self.l.update(event);
        self.f.update(event);
        self.lambda.update(event);
    }

    pub fn snapshot(&self) -> Metrics {
        Metrics {
            i: self.i.value(),
            e: self.e.value(),
            c: self.c.value(),
            a: self.a.value(),
            r: self.r.value(),
            p: self.p.value(),
            s: self.s.value(),
            d: self.d.value(),
            x: self.x.value(),
            b: self.b.value(),
            l: self.l.value(),
            f: self.f.value(),
            lambda: self.lambda.value(),
        }
    }
}

pub use alignment::Alignment;
pub use benefit::Benefit;
pub use correctness::Correctness;
pub use determinism::Determinism;
pub use efficiency::Efficiency;
pub use execution::Execution;
pub use future::FutureProof;
pub use intelligence::Intelligence;
pub use learning::Learning;
pub use love::Love;
pub use performance::Performance;
pub use robustness::Robustness;
pub use scalability::Scalability;
