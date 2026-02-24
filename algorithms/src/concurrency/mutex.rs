use std::sync::{Arc, Mutex};

pub fn with_mutex<T, F: FnOnce(&mut T)>(data: Arc<Mutex<T>>, f: F) {
    let mut guard = data.lock().unwrap();
    f(&mut *guard);
}
