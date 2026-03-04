//! Interval analysis for type domain narrowing.
//!
//! Variables:
//!   domains : &[Interval]   — per-variable integer intervals [lo, hi]
//!   constraints : &[(usize, usize, IntervalConstraint)] — (i, j, kind)
//!
//! Equations:
//!   narrow(D_i, D_j, Lt)  => D_i.hi = min(D_i.hi, D_j.hi - 1)
//!   narrow(D_i, D_j, Lte) => D_i.hi = min(D_i.hi, D_j.hi)
//!   narrow(D_i, D_j, Eq)  => D_i = D_i ∩ D_j
//!   fixed_point: repeat narrowing until no domain changes
//!
//! Used by: type_solver to narrow generic parameter domains beyond AC-3 equality.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub lo: i64,
    pub hi: i64,
}

impl Interval {
    pub fn full() -> Self { Self { lo: i64::MIN, hi: i64::MAX } }
    pub fn empty() -> Self { Self { lo: 1, hi: 0 } }
    pub fn is_empty(&self) -> bool { self.lo > self.hi }
    pub fn intersect(&self, other: &Self) -> Self {
        Self { lo: self.lo.max(other.lo), hi: self.hi.min(other.hi) }
    }
}

#[derive(Debug, Clone)]
pub enum IntervalConstraint {
    /// D_i == D_j
    Eq,
    /// D_i <= D_j
    Lte,
    /// D_i < D_j
    Lt,
    /// D_i subset of D_j
    Subset,
}

/// Narrow intervals to fixed point under constraints.
/// Returns false if any domain becomes empty (contradiction).
pub fn interval_narrowing(
    domains: &mut Vec<Interval>,
    constraints: &[(usize, usize, IntervalConstraint)],
) -> bool {
    let mut changed = true;
    while changed {
        changed = false;
        for (i, j, kind) in constraints {
            let (i, j) = (*i, *j);
            if i >= domains.len() || j >= domains.len() { continue; }
            let new_i = match kind {
                IntervalConstraint::Eq => {
                    domains[i].intersect(&domains[j].clone())
                }
                IntervalConstraint::Lte => {
                    let hi = domains[i].hi.min(domains[j].hi);
                    Interval { lo: domains[i].lo, hi }
                }
                IntervalConstraint::Lt => {
                    let hi = domains[i].hi.min(domains[j].hi.saturating_sub(1));
                    Interval { lo: domains[i].lo, hi }
                }
                IntervalConstraint::Subset => {
                    domains[i].intersect(&domains[j].clone())
                }
            };
            if new_i != domains[i] {
                domains[i] = new_i;
                changed = true;
            }
            if domains[i].is_empty() {
                return false;
            }
        }
    }
    true
}
