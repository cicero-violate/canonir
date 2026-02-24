pub fn rabin_karp(text: &str, pattern: &str) -> Vec<usize> {
    const BASE: u64 = 256;
    const MOD: u64 = 1_000_000_007;

    let t = text.as_bytes();
    let p = pattern.as_bytes();
    let n = t.len();
    let m = p.len();
    if m == 0 || m > n {
        return vec![];
    }

    let mut hash_p = 0;
    let mut hash_t = 0;
    let mut power = 1;

    for i in 0..m {
        hash_p = (hash_p * BASE + p[i] as u64) % MOD;
        hash_t = (hash_t * BASE + t[i] as u64) % MOD;
        if i < m - 1 {
            power = (power * BASE) % MOD;
        }
    }

    let mut result = Vec::new();
    for i in 0..=n - m {
        if hash_p == hash_t && &t[i..i + m] == p {
            result.push(i);
        }
        if i < n - m {
            hash_t = (MOD + hash_t - (t[i] as u64 * power) % MOD) % MOD;
            hash_t = (hash_t * BASE + t[i + m] as u64) % MOD;
        }
    }
    result
}
