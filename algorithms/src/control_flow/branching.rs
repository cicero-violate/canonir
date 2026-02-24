pub fn branching<T>(state: T, predicate: fn(&T) -> bool, t: fn(T) -> T, f: fn(T) -> T) -> T {
    if predicate(&state) {
        t(state)
    } else {
        f(state)
    }
}
