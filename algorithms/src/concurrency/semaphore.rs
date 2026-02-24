use std::sync::{Arc, Condvar, Mutex};

pub struct Semaphore {
    count: Mutex<usize>,
    cvar: Condvar,
}

impl Semaphore {
    pub fn new(count: usize) -> Arc<Self> {
        Arc::new(Self { count: Mutex::new(count), cvar: Condvar::new() })
    }

    pub fn acquire(self: &Arc<Self>) {
        let mut count = self.count.lock().unwrap();
        while *count == 0 {
            count = self.cvar.wait(count).unwrap();
        }
        *count -= 1;
    }

    pub fn release(self: &Arc<Self>) {
        let mut count = self.count.lock().unwrap();
        *count += 1;
        self.cvar.notify_one();
    }
}
