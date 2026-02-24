pub fn sequential<T>(mut state: T, steps: Vec<fn(T) -> T>) -> T {
    for step in steps {
        state = step(state);
    }
    state
}
