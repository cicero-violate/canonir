use std::sync::atomic::{AtomicUsize, Ordering};

pub fn compare_and_swap(atom: &AtomicUsize, current: usize, new: usize) -> bool {
    atom.compare_exchange(current, new, Ordering::SeqCst, Ordering::SeqCst).is_ok()
}
