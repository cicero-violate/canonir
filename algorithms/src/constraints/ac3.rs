use std::collections::VecDeque;

pub type Domain = Vec<i32>;
pub type ConstraintFn = Box<dyn Fn(i32, i32) -> bool + Send + Sync>;

#[derive(Default)]
pub struct ConstraintGraph {
    /// constraints[(i, j)] = predicate for (xi, xj)
    pub constraints: std::collections::HashMap<(usize, usize), ConstraintFn>,
}

impl ConstraintGraph {
    pub fn add_constraint<F>(&mut self, i: usize, j: usize, f: F)
    where
        F: Fn(i32, i32) -> bool + Send + Sync + 'static,
    {
        self.constraints.insert((i, j), Box::new(f));
    }

    pub fn constraint(&self, i: usize, j: usize) -> Option<&ConstraintFn> {
        self.constraints.get(&(i, j))
    }
}

/// AC-3 arc consistency algorithm.
/// Returns false if a domain becomes empty.
pub fn ac3(domains: &mut [Domain], graph: &ConstraintGraph) -> bool {
    let mut queue = VecDeque::new();
    for (&(i, j), _) in &graph.constraints {
        queue.push_back((i, j));
    }

    while let Some((i, j)) = queue.pop_front() {
        if revise(domains, graph, i, j) {
            if domains[i].is_empty() {
                return false;
            }
            for (&(k, l), _) in &graph.constraints {
                if l == i && k != j {
                    queue.push_back((k, i));
                }
            }
        }
    }
    true
}

/// GPU-ready arc constraints + domain layout.
#[derive(Debug, Clone)]
pub struct GpuArcConstraints {
    pub arc_i: Vec<i32>,
    pub arc_j: Vec<i32>,
    pub arc_dom_i_len: Vec<i32>,
    pub arc_dom_j_len: Vec<i32>,
    pub arc_constraint_offset: Vec<i32>,
    pub domain_offsets: Vec<i32>,
    pub domain_active: Vec<i32>,
    pub constraint_values: Vec<u8>,
    pub domain_values: Vec<i32>,
    pub domain_value_offsets: Vec<i32>,
}

impl GpuArcConstraints {
    pub fn new(
        domains: &[Domain],
        graph: &ConstraintGraph,
    ) -> Self {
        let mut domain_offsets = Vec::with_capacity(domains.len() + 1);
        domain_offsets.push(0i32);
        let mut domain_value_offsets = Vec::with_capacity(domains.len() + 1);
        domain_value_offsets.push(0i32);
        let mut domain_values = Vec::new();
        for d in domains {
            let next = domain_offsets.last().copied().unwrap_or(0) + d.len() as i32;
            domain_offsets.push(next);
            for &v in d {
                domain_values.push(v);
            }
            let next_val = domain_value_offsets.last().copied().unwrap_or(0) + d.len() as i32;
            domain_value_offsets.push(next_val);
        }
        let mut domain_active = Vec::new();
        for d in domains {
            for _ in d {
                domain_active.push(1i32);
            }
        }

        let mut arc_i = Vec::new();
        let mut arc_j = Vec::new();
        let mut arc_dom_i_len = Vec::new();
        let mut arc_dom_j_len = Vec::new();
        let mut arc_constraint_offset = Vec::new();
        let mut constraint_values = Vec::new();

        for (&(i, j), pred) in &graph.constraints {
            let di = domains[i].len();
            let dj = domains[j].len();
            let offset = constraint_values.len();
            for &xi in &domains[i] {
                for &xj in &domains[j] {
                    constraint_values.push(if pred(xi, xj) { 1 } else { 0 });
                }
            }
            arc_i.push(i as i32);
            arc_j.push(j as i32);
            arc_dom_i_len.push(di as i32);
            arc_dom_j_len.push(dj as i32);
            arc_constraint_offset.push(offset as i32);
        }

        Self {
            arc_i,
            arc_j,
            arc_dom_i_len,
            arc_dom_j_len,
            arc_constraint_offset,
            domain_offsets,
            domain_active,
            constraint_values,
            domain_values,
            domain_value_offsets,
        }
    }

    /// Validate internal buffer sizes and offsets.
    pub fn validate(&self) -> Result<(), String> {
        let vars = self.domain_offsets.len().saturating_sub(1);
        if self.domain_value_offsets.len() != self.domain_offsets.len() {
            return Err("domain_value_offsets length mismatch".into());
        }
        let total = *self.domain_offsets.last().unwrap_or(&0) as usize;
        if total != self.domain_active.len() {
            return Err("domain_active length mismatch".into());
        }
        if total != self.domain_values.len() {
            return Err("domain_values length mismatch".into());
        }
        if self.arc_i.len() != self.arc_j.len()
            || self.arc_i.len() != self.arc_dom_i_len.len()
            || self.arc_i.len() != self.arc_dom_j_len.len()
            || self.arc_i.len() != self.arc_constraint_offset.len()
        {
            return Err("arc array length mismatch".into());
        }
        let arc_count = self.arc_i.len();
        for a in 0..arc_count {
            let i = self.arc_i[a] as usize;
            let j = self.arc_j[a] as usize;
            if i >= vars || j >= vars {
                return Err(format!("arc {} has invalid variable index", a));
            }
            let di = self.arc_dom_i_len[a] as usize;
            let dj = self.arc_dom_j_len[a] as usize;
            let off = self.arc_constraint_offset[a] as usize;
            let needed = di.saturating_mul(dj);
            if off + needed > self.constraint_values.len() {
                return Err(format!("arc {} constraint matrix out of bounds", a));
            }
            let i_len = (self.domain_offsets[i + 1] - self.domain_offsets[i]) as usize;
            let j_len = (self.domain_offsets[j + 1] - self.domain_offsets[j]) as usize;
            if di != i_len || dj != j_len {
                return Err(format!("arc {} domain length mismatch", a));
            }
        }
        Ok(())
    }

    /// Convert domain_active mask back into per-variable value lists.
    pub fn to_domains(&self) -> Vec<Domain> {
        let vars = self.domain_offsets.len().saturating_sub(1);
        let mut out = Vec::with_capacity(vars);
        for i in 0..vars {
            let start = self.domain_offsets[i] as usize;
            let end = self.domain_offsets[i + 1] as usize;
            let mut dom = Vec::new();
            for idx in start..end {
                if self.domain_active[idx] != 0 {
                    dom.push(self.domain_values[idx]);
                }
            }
            out.push(dom);
        }
        out
    }
}

#[cfg(feature = "cuda")]
unsafe extern "C" {
    fn gpu_ac3_revise(
        arc_count: i32,
        var_count: i32,
        arc_i: *const i32,
        arc_j: *const i32,
        arc_dom_i_len: *const i32,
        arc_dom_j_len: *const i32,
        arc_constraint_offset: *const i32,
        domain_offsets: *const i32,
        domain_active: *mut i32,
        constraint_values: *const u8,
    ) -> i32;
}

/// GPU AC-3 over arc constraints with dense per-arc constraint matrices.
/// Domains are represented by index (0..len-1) and tracked via domain_active.
#[cfg(feature = "cuda")]
pub fn ac3_gpu(
    arc_i: &[i32],
    arc_j: &[i32],
    arc_dom_i_len: &[i32],
    arc_dom_j_len: &[i32],
    arc_constraint_offset: &[i32],
    domain_offsets: &[i32],
    domain_active: &mut [i32],
    constraint_values: &[u8],
) -> bool {
    let var_count = domain_offsets.len().saturating_sub(1) as i32;
    unsafe {
        gpu_ac3_revise(
            arc_i.len() as i32,
            var_count,
            arc_i.as_ptr(),
            arc_j.as_ptr(),
            arc_dom_i_len.as_ptr(),
            arc_dom_j_len.as_ptr(),
            arc_constraint_offset.as_ptr(),
            domain_offsets.as_ptr(),
            domain_active.as_mut_ptr(),
            constraint_values.as_ptr(),
        ) != 0
    }
}

/// Convenience: build GPU buffers from domains + graph, run AC-3 on GPU,
/// and return the pruned domains.
#[cfg(feature = "cuda")]
pub fn ac3_gpu_apply(domains: &[Domain], graph: &ConstraintGraph) -> Option<Vec<Domain>> {
    let mut gpu = GpuArcConstraints::new(domains, graph);
    if gpu.validate().is_err() {
        return None;
    }
    let _changed = ac3_gpu(
        &gpu.arc_i,
        &gpu.arc_j,
        &gpu.arc_dom_i_len,
        &gpu.arc_dom_j_len,
        &gpu.arc_constraint_offset,
        &gpu.domain_offsets,
        &mut gpu.domain_active,
        &gpu.constraint_values,
    );
    Some(gpu.to_domains())
}

fn revise(domains: &mut [Domain], graph: &ConstraintGraph, i: usize, j: usize) -> bool {
    let Some(pred) = graph.constraint(i, j) else {
        return false;
    };
    let mut revised = false;
    let mut keep = Vec::new();
    for &x in &domains[i] {
        let mut ok = false;
        for &y in &domains[j] {
            if pred(x, y) {
                ok = true;
                break;
            }
        }
        if ok {
            keep.push(x);
        } else {
            revised = true;
        }
    }
    if revised {
        domains[i] = keep;
    }
    revised
}
