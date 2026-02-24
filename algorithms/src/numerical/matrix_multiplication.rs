pub fn matrix_multiply(a: &[Vec<i64>], b: &[Vec<i64>]) -> Vec<Vec<i64>> {
    let n = a.len();
    let m = b[0].len();
    let mut result = vec![vec![0; m]; n];

    for i in 0..n {
        for k in 0..b.len() {
            for j in 0..m {
                result[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    result
}
