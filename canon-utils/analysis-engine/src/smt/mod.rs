use z3::{Config, Solver};

pub struct SmtSession {
    timeout_ms: u64,
}

impl SmtSession {
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }

    pub fn solver(&self) -> Solver {
        Solver::new()
    }

    pub fn run<T: Send + Sync, F: FnOnce() -> T + Send + Sync>(&self, f: F) -> T {
        let mut cfg = Config::new();
        cfg.set_timeout_msec(self.timeout_ms);
        z3::with_z3_config(&cfg, f)
    }
}

pub mod encoder;
pub mod equivalence;
pub mod invariants;
pub mod reachability;
pub mod repair;
