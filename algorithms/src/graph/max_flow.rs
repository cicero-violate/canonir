use std::collections::VecDeque;

#[derive(Clone, Debug)]
struct Edge {
    to: usize,
    rev: usize,
    cap: i64,
}

/// Maximum flow using push-relabel (preflow) with gap-less heuristics.
///
/// `edges` is a list of (u, v, capacity).
pub fn max_flow_push_relabel(node_count: usize, edges: &[(usize, usize, i64)], source: usize, sink: usize) -> i64 {
    if node_count == 0 || source >= node_count || sink >= node_count || source == sink {
        return 0;
    }
    let mut g = vec![Vec::<Edge>::new(); node_count];
    for &(u, v, cap) in edges {
        if u >= node_count || v >= node_count || cap <= 0 {
            continue;
        }
        add_edge(&mut g, u, v, cap);
    }

    let mut height = vec![0usize; node_count];
    let mut excess = vec![0i64; node_count];
    height[source] = node_count;
    for i in 0..g[source].len() {
        let cap = g[source][i].cap;
        if cap > 0 {
            let v = g[source][i].to;
            g[source][i].cap = 0;
            let rev = g[source][i].rev;
            g[v][rev].cap += cap;
            excess[v] += cap;
        }
    }

    let mut active = VecDeque::new();
    for v in 0..node_count {
        if v != source && v != sink && excess[v] > 0 {
            active.push_back(v);
        }
    }

    while let Some(u) = active.pop_front() {
        discharge(u, &mut g, &mut height, &mut excess, source, sink, &mut active);
    }

    excess[sink]
}

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_max_flow_push_relabel(
        v: i32,
        e: i32,
        src: *const i32,
        dst: *const i32,
        cap: *const i64,
        source: i32,
        sink: i32,
    ) -> i64;
}

/// GPU entrypoint (currently CPU fallback compiled by nvcc).
#[cfg(feature = "cuda")]
pub fn max_flow_gpu(node_count: usize, edges: &[(usize, usize, i64)], source: usize, sink: usize) -> i64 {
    let mut src = Vec::with_capacity(edges.len());
    let mut dst = Vec::with_capacity(edges.len());
    let mut cap = Vec::with_capacity(edges.len());
    for &(u, v, c) in edges {
        src.push(u as i32);
        dst.push(v as i32);
        cap.push(c as i64);
    }
    unsafe {
        gpu_max_flow_push_relabel(
            node_count as i32,
            edges.len() as i32,
            src.as_ptr(),
            dst.as_ptr(),
            cap.as_ptr(),
            source as i32,
            sink as i32,
        )
    }
}

fn add_edge(g: &mut [Vec<Edge>], u: usize, v: usize, cap: i64) {
    let rev_u = g[v].len();
    let rev_v = g[u].len();
    g[u].push(Edge { to: v, rev: rev_u, cap });
    g[v].push(Edge { to: u, rev: rev_v, cap: 0 });
}

fn discharge(
    u: usize,
    g: &mut [Vec<Edge>],
    height: &mut [usize],
    excess: &mut [i64],
    source: usize,
    sink: usize,
    active: &mut VecDeque<usize>,
) {
    while excess[u] > 0 {
        let mut pushed = false;
        for i in 0..g[u].len() {
            let v = g[u][i].to;
            if g[u][i].cap > 0 && height[u] == height[v] + 1 {
                let delta = excess[u].min(g[u][i].cap);
                g[u][i].cap -= delta;
                let rev = g[u][i].rev;
                g[v][rev].cap += delta;
                excess[u] -= delta;
                excess[v] += delta;
                if v != source && v != sink && excess[v] == delta {
                    active.push_back(v);
                }
                if excess[u] == 0 {
                    pushed = true;
                    break;
                }
            }
        }
        if pushed && excess[u] == 0 {
            break;
        }
        relabel(u, g, height);
    }
}

fn relabel(u: usize, g: &[Vec<Edge>], height: &mut [usize]) {
    let mut min_h = usize::MAX;
    for e in &g[u] {
        if e.cap > 0 {
            min_h = min_h.min(height[e.to]);
        }
    }
    if min_h != usize::MAX {
        height[u] = min_h + 1;
    }
}
