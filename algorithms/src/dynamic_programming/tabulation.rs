pub fn fib_tab(n: usize) -> u64 {
    if n <= 1 {
        return n as u64;
    }
    let mut dp = vec![0u64; n + 1];
    dp[1] = 1;
    for i in 2..=n {
        dp[i] = dp[i - 1] + dp[i - 2];
    }
    dp[n]
}
