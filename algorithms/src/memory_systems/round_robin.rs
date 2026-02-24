pub fn round_robin<T>(tasks: &mut [T], step: fn(&mut T)) {
    for task in tasks.iter_mut() {
        step(task);
    }
}
