/// Defines invariants for optimization and graph algorithms
pub trait Invariant<T> {
    /// Checks if a value satisfies the invariant
    fn check(&self, value: &T) -> bool;
}

/// Example invariant: value must be non-negative
pub struct NonNegative;

impl Invariant<i64> for NonNegative {
    fn check(&self, value: &i64) -> bool {
        *value >= 0
    }
}

/// Example invariant: a vector must be sorted in ascending order
pub struct Sorted;

impl Invariant<Vec<u64>> for Sorted {
    fn check(&self, value: &Vec<u64>) -> bool {
        value.windows(2).all(|w| w[0] <= w[1])
    }
}
