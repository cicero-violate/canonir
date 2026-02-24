/// Invariant trait for graph algorithm checks
pub trait Invariant<T> {
    fn check(&self, value: &T) -> bool;
}

/// Non-negative invariant for integers
pub struct NonNegative;

impl Invariant<i64> for NonNegative {
    fn check(&self, value: &i64) -> bool {
        *value >= 0
    }
}
