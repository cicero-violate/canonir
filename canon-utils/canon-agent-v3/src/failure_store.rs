#[derive(Debug, Clone)]
pub struct FailureStoreFailureStats {
    pub total: usize,
    pub cycle: usize,
    pub deadlock: usize,
    pub failure_pattern_rate: f64,
    pub cycle_frequency: f64,
    pub deadlock_rate: f64,
}
