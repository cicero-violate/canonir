pub fn kmp_search(text: &str, pattern: &str) -> Vec<usize> {
    let t = text.as_bytes();
    let p = pattern.as_bytes();
    if p.is_empty() {
        return vec![];
    }

    let mut lps = vec![0; p.len()];
    {
        let mut len = 0;
        for i in 1..p.len() {
            while len > 0 && p[i] != p[len] {
                len = lps[len - 1];
            }
            if p[i] == p[len] {
                len += 1;
                lps[i] = len;
            }
        }
    }

    let mut res = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < t.len() {
        if t[i] == p[j] {
            i += 1;
            j += 1;
            if j == p.len() {
                res.push(i - j);
                j = lps[j - 1];
            }
        } else if j > 0 {
            j = lps[j - 1];
        } else {
            i += 1;
        }
    }
    res
}
