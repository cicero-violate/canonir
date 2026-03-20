pub fn beam_search<S, Expand, Score>(initial: Vec<S>, beam_width: usize, depth: usize, mut expand: Expand, mut score: Score) -> Option<S>
where
    S: Clone,
    Expand: FnMut(&S, usize) -> Vec<S>,
    Score: FnMut(&S) -> i64,
{
    if initial.is_empty() || beam_width == 0 {
        return None;
    }

    let mut beam = initial;
    for level in 0..depth {
        let mut next = Vec::new();
        for state in &beam {
            next.extend(expand(state, level));
        }
        if next.is_empty() {
            break;
        }
        next.sort_by_key(|s| std::cmp::Reverse(score(s)));
        next.truncate(beam_width);
        beam = next;
    }

    beam.into_iter().max_by_key(|s| score(s))
}
