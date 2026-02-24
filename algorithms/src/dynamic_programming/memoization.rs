use std::collections::HashMap;

pub fn fib_memo(n: u64, memo: &mut HashMap<u64, u64>) -> u64 {
    if n <= 1 {
        return n;
    }
    if let Some(&v) = memo.get(&n) {
        return v;
    }
    let val = fib_memo(n - 1, memo) + fib_memo(n - 2, memo);
    memo.insert(n, val);
    val
}
