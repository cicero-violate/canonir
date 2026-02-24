pub fn branch_and_bound(weights: &[u64], capacity: u64) -> u64 {
    fn helper(weights: &[u64], i: usize, cap: u64) -> u64 {
        if i == weights.len() || cap == 0 {
            return 0;
        }
        if weights[i] > cap {
            helper(weights, i + 1, cap)
        } else {
            let include = weights[i] + helper(weights, i + 1, cap - weights[i]);
            let exclude = helper(weights, i + 1, cap);
            include.max(exclude)
        }
    }
    helper(weights, 0, capacity)
}
