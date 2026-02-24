pub fn linear_search<T: PartialEq>(arr: &[T], target: &T) -> Option<usize> {
    for (i, v) in arr.iter().enumerate() {
        if v == target {
            return Some(i);
        }
    }
    None
}
