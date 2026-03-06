pub fn ddmin<T, F>(items: &[T], mut fails: F) -> Vec<T>
where
    T: Clone,
    F: FnMut(&[T]) -> bool,
{
    if items.is_empty() || !fails(items) {
        return Vec::new();
    }

    let mut current: Vec<T> = items.to_vec();
    let mut n = 2usize;

    while current.len() >= 2 {
        let chunk = current.len().div_ceil(n);
        let mut reduced = false;

        let mut i = 0usize;
        while i < current.len() {
            let end = usize::min(i + chunk, current.len());
            let mut candidate = Vec::with_capacity(current.len() - (end - i));
            candidate.extend_from_slice(&current[..i]);
            candidate.extend_from_slice(&current[end..]);

            if fails(&candidate) {
                current = candidate;
                n = usize::max(2, n - 1);
                reduced = true;
                break;
            }
            i = end;
        }

        if !reduced {
            if n >= current.len() {
                break;
            }
            n = usize::min(current.len(), n * 2);
        }
    }

    current
}
