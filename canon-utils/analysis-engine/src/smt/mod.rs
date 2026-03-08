use z3::Solver;

pub struct SmtSession;

impl SmtSession {
    pub fn new() -> Self {
        Self
    }

    pub fn solver(&self) -> Solver {
        Solver::new()
    }
}

pub mod encoder;
pub mod equivalence;
pub mod invariants;
pub mod reachability;
pub mod repair;
