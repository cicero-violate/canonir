pub fn looping<T>(mut state: T, predicate: fn(&T) -> bool, step: fn(T) -> T) -> T {
    while predicate(&state) {
        state = step(state);
    }
    state
}
