pub fn binary_search<T: Ord>(arr: &[T], target: &T) -> Option<usize> {
    let (mut l, mut r) = (0, arr.len());
    while l < r {
        let m = (l + r) / 2;
        if &arr[m] == target {
            return Some(m);
        }
        if &arr[m] < target {
            l = m + 1;
        } else {
            r = m;
        }
    }
    None
}
