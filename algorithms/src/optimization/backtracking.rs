pub fn subsets<T: Clone>(set: &[T]) -> Vec<Vec<T>> {
    fn backtrack<T: Clone>(set: &[T], index: usize, current: &mut Vec<T>, result: &mut Vec<Vec<T>>) {
        if index == set.len() {
            result.push(current.clone());
            return;
        }
        backtrack(set, index + 1, current, result);
        current.push(set[index].clone());
        backtrack(set, index + 1, current, result);
        current.pop();
    }

    let mut result = Vec::new();
    backtrack(set, 0, &mut Vec::new(), &mut result);
    result
}
