use self::cache::ProofCache;
use std::sync::Mutex;
use z3::{Config, Context, Solver};

pub struct SmtSession {
    ctx: Context,
    cache: Mutex<ProofCache>,
}

impl SmtSession {
    pub fn new(timeout_ms: u64, cache_path: std::path::PathBuf, clear_cache: bool) -> Self {
        let mut cfg = Config::new();
        cfg.set_timeout_msec(timeout_ms);
        let ctx = Context::new(&cfg);
        Self {
            ctx,
            cache: Mutex::new(ProofCache::new(cache_path, clear_cache)),
        }
    }

    pub fn solver(&self) -> Solver<'_> {
        Solver::new(&self.ctx)
    }

    pub fn ctx(&self) -> &Context {
        &self.ctx
    }

    pub fn cache(&self) -> &Mutex<ProofCache> {
        &self.cache
    }
}

pub mod encoder;
pub mod equivalence;
pub mod invariants;
pub mod reachability;
pub mod repair;
pub mod cache;
